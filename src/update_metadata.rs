use hbb_common::{
    anyhow::{anyhow, Context},
    base64::{engine::general_purpose::STANDARD, Engine as _},
    sodiumoxide::{
        self,
        crypto::sign::{verify_detached, PublicKey, Signature},
    },
    ResultType,
};
use serde_derive::Deserialize;
use std::sync::OnceLock;

mod artifact;

#[cfg(target_os = "macos")]
mod offline;

use artifact::{validate_artifact, validate_artifact_file_name, ArtifactUrlPolicy};

#[cfg(target_os = "macos")]
pub(crate) use offline::{verify_offline_update_metadata, OfflineUpdateMetadataRequirements};

const UPDATE_METADATA_SIGNATURE_CONTEXT: &[u8] = b"RustDesk update metadata v1\n";
const UPDATE_APP: &str = "rustdesk";
pub(crate) const PUBLIC_UPDATE_PACKAGE_ID: &str = "rustdesk";
const METADATA_SCHEMA_VERSION: u32 = 1;
const SIGNATURE_SCHEMA_VERSION: u32 = 1;
const SIGNATURE_ALGORITHM: &str = "ed25519";

/// BetterDesk client appcast (`appcast/<channel>.json`).
///
/// A distinct app id, key id and signature context means a manifest signed for upstream
/// RustDesk can never be accepted as a BetterDesk update, and vice versa.
const BETTERDESK_SIGNATURE_CONTEXT: &[u8] = b"BetterDesk client update metadata v1\n";
const BETTERDESK_APP: &str = "betterdesk-client";
const BETTERDESK_KEY_ID: &str = "2026-ed25519-betterdesk-client";

/// Which product's manifest is being verified.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum UpdateTrust {
    /// Upstream RustDesk release (manual/offline update path only).
    RustDesk,
    /// BetterDesk appcast feed produced by `scripts/build_update_feed.py`.
    BetterDesk,
}

impl UpdateTrust {
    fn signature_context(&self) -> &'static [u8] {
        match self {
            UpdateTrust::RustDesk => UPDATE_METADATA_SIGNATURE_CONTEXT,
            UpdateTrust::BetterDesk => BETTERDESK_SIGNATURE_CONTEXT,
        }
    }

    fn app(&self) -> &'static str {
        match self {
            UpdateTrust::RustDesk => UPDATE_APP,
            UpdateTrust::BetterDesk => BETTERDESK_APP,
        }
    }
}

const TRUSTED_UPDATE_KEYS: &[TrustedUpdateKey] = &[TrustedUpdateKey {
    key_id: "2026-ed25519-main",
    algorithm: SIGNATURE_ALGORITHM,
    public_key: [
        118, 252, 116, 215, 49, 217, 220, 109, 145, 119, 210, 101, 225, 198, 63, 2, 41, 127, 197,
        127, 52, 251, 6, 204, 125, 117, 96, 204, 187, 166, 85, 59,
    ],
}];

/// BetterDesk update signing keys, baked in at build time.
///
/// Committed as a public key only (`res/betterdesk/client-update-signing.pub`), generated
/// with `scripts/generate_custom_client_signing_key.py --kind update`. An empty or
/// unparsable value disables BetterDesk update *verification*, which fails closed: the
/// client then reports that the feed cannot be trusted instead of installing anything.
fn betterdesk_update_key() -> Option<[u8; 32]> {
    const PUBKEY: &str = include_str!("../res/betterdesk/client-update-signing.pub");
    let encoded = PUBKEY.trim();
    if encoded.is_empty() {
        return None;
    }
    let decoded = STANDARD.decode(encoded).ok()?;
    let bytes: [u8; 32] = decoded.try_into().ok()?;
    (STANDARD.encode(bytes) == encoded).then_some(bytes)
}

#[derive(Deserialize)]
struct UpdateMetadata {
    schema_version: u32,
    app: String,
    package_id: String,
    version: String,
    release_id: String,
    #[serde(rename = "published_at")]
    _published_at: String,
    signature_key_id: String,
    artifacts: Vec<UpdateArtifact>,
}

#[derive(Deserialize)]
struct UpdateArtifact {
    platform: String,
    arch: String,
    format: String,
    url: String,
    file_name: String,
    size: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct UpdateSignature {
    schema_version: u32,
    algorithm: String,
    key_id: String,
    signature: String,
}

struct TrustedUpdateKey {
    key_id: &'static str,
    algorithm: &'static str,
    public_key: [u8; 32],
}

#[derive(Clone, Copy)]
pub(crate) struct UpdateArtifactQuery<'a> {
    pub platform: &'a str,
    pub arch: &'a str,
    pub format: &'a str,
}

#[derive(Clone, Copy)]
pub(crate) struct UpdateMetadataRequirements<'a> {
    pub expected_version: &'a str,
    pub expected_release_id: &'a str,
    pub expected_artifact_url_prefix: &'a str,
    pub artifact: UpdateArtifactQuery<'a>,
    /// Package id the manifest must declare (e.g. `betterdesk-desktop`).
    pub expected_package_id: &'a str,
}

#[derive(Clone)]
pub(crate) struct VerifiedUpdateArtifact {
    pub version: String,
    pub url: String,
    #[cfg(target_os = "macos")]
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
}

struct VerificationOptions<'a> {
    requirements: MetadataRequirements<'a>,
    expected_package_id: &'a str,
    artifact_url_policy: ArtifactUrlPolicy<'a>,
    trusted_keys: &'a [TrustedUpdateKey],
    trust: UpdateTrust,
}

#[derive(Clone, Copy)]
struct MetadataRequirements<'a> {
    expected_version: &'a str,
    expected_release_id: &'a str,
    artifact: UpdateArtifactQuery<'a>,
}

fn verify_update_metadata_with_options(
    metadata_bytes: &[u8],
    signature_bytes: &[u8],
    options: VerificationOptions<'_>,
) -> ResultType<VerifiedUpdateArtifact> {
    let update_signature: UpdateSignature =
        serde_json::from_slice(signature_bytes).context("invalid update signature JSON")?;
    validate_signature_schema(&update_signature)?;
    let signature = decode_signature(&update_signature.signature)?;
    let trusted_key = options
        .trusted_keys
        .iter()
        .find(|key| {
            key.key_id == update_signature.key_id && key.algorithm == update_signature.algorithm
        })
        .ok_or_else(|| anyhow!("unsupported update signature key"))?;
    verify_metadata_signature(metadata_bytes, &signature, trusted_key, options.trust)?;
    let metadata: UpdateMetadata =
        serde_json::from_slice(metadata_bytes).context("invalid update metadata JSON")?;
    validate_metadata(&metadata, &update_signature, &options)?;
    let artifact = select_artifact(&metadata, &options.requirements.artifact)?;
    validate_artifact(artifact, options.artifact_url_policy)?;
    Ok(VerifiedUpdateArtifact {
        version: metadata.version.clone(),
        url: artifact.url.clone(),
        #[cfg(target_os = "macos")]
        file_name: artifact.file_name.clone(),
        size: artifact.size,
        sha256: artifact.sha256.clone(),
    })
}

pub(crate) fn verify_update_metadata(
    metadata_bytes: &[u8],
    signature_bytes: &[u8],
    requirements: UpdateMetadataRequirements<'_>,
) -> ResultType<VerifiedUpdateArtifact> {
    let artifact_url_policy =
        ArtifactUrlPolicy::ExactPrefix(requirements.expected_artifact_url_prefix);
    let metadata_requirements = MetadataRequirements {
        expected_version: requirements.expected_version,
        expected_release_id: requirements.expected_release_id,
        artifact: requirements.artifact,
    };
    verify_update_metadata_with_options(
        metadata_bytes,
        signature_bytes,
        VerificationOptions {
            requirements: metadata_requirements,
            expected_package_id: requirements.expected_package_id,
            artifact_url_policy,
            trusted_keys: TRUSTED_UPDATE_KEYS,
            trust: UpdateTrust::RustDesk,
        },
    )
}

/// Verify a BetterDesk appcast manifest (`appcast/<channel>.json`) and its `.sig`.
///
/// Fails closed when no BetterDesk update key is embedded, so a build without a key can
/// never install a feed it cannot authenticate.
pub(crate) fn verify_betterdesk_update_metadata(
    metadata_bytes: &[u8],
    signature_bytes: &[u8],
    requirements: UpdateMetadataRequirements<'_>,
) -> ResultType<VerifiedUpdateArtifact> {
    let Some(public_key) = betterdesk_update_key() else {
        return Err(anyhow!(
            "this build has no BetterDesk update key; \
             the update feed cannot be authenticated"
        ));
    };
    static BETTERDESK_KEYS: OnceLock<[TrustedUpdateKey; 1]> = OnceLock::new();
    let trusted_keys = BETTERDESK_KEYS.get_or_init(|| {
        [TrustedUpdateKey {
            key_id: BETTERDESK_KEY_ID,
            algorithm: SIGNATURE_ALGORITHM,
            public_key,
        }]
    });
    let signature: UpdateSignature =
        serde_json::from_slice(signature_bytes).context("invalid update signature JSON")?;
    if signature.key_id != BETTERDESK_KEY_ID {
        return Err(anyhow!("update feed is not signed for BetterDesk"));
    }
    let artifact_url_policy =
        ArtifactUrlPolicy::ExactPrefix(requirements.expected_artifact_url_prefix);
    let metadata_requirements = MetadataRequirements {
        expected_version: requirements.expected_version,
        expected_release_id: requirements.expected_release_id,
        artifact: requirements.artifact,
    };
    verify_update_metadata_with_options(
        metadata_bytes,
        signature_bytes,
        VerificationOptions {
            requirements: metadata_requirements,
            expected_package_id: requirements.expected_package_id,
            artifact_url_policy,
            trusted_keys,
            trust: UpdateTrust::BetterDesk,
        },
    )
}

fn validate_signature_schema(signature: &UpdateSignature) -> ResultType<()> {
    if signature.schema_version != SIGNATURE_SCHEMA_VERSION {
        return Err(anyhow!("unsupported update signature schema version"));
    }
    if signature.algorithm != SIGNATURE_ALGORITHM {
        return Err(anyhow!("unsupported update signature algorithm"));
    }
    Ok(())
}

fn decode_signature(encoded: &str) -> ResultType<Signature> {
    let decoded = STANDARD
        .decode(encoded)
        .context("invalid update signature base64")?;
    if decoded.len() != 64 || STANDARD.encode(&decoded) != encoded {
        return Err(anyhow!("invalid update signature length or encoding"));
    }
    let mut signature = [0u8; 64];
    signature.copy_from_slice(&decoded);
    Signature::from_bytes(&signature).map_err(|_| anyhow!("invalid update signature bytes"))
}

fn verify_metadata_signature(
    metadata_bytes: &[u8],
    signature: &Signature,
    trusted_key: &TrustedUpdateKey,
    trust: UpdateTrust,
) -> ResultType<()> {
    init_sodiumoxide()?;
    let context = trust.signature_context();
    let mut signed_bytes = Vec::with_capacity(context.len() + metadata_bytes.len());
    signed_bytes.extend_from_slice(context);
    signed_bytes.extend_from_slice(metadata_bytes);
    if !verify_detached(signature, &signed_bytes, &PublicKey(trusted_key.public_key)) {
        return Err(anyhow!("invalid update metadata signature"));
    }
    Ok(())
}

fn init_sodiumoxide() -> ResultType<()> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    let result = INIT.get_or_init(|| {
        sodiumoxide::init().map_err(|_| "failed to initialize sodiumoxide".to_string())
    });
    match result {
        Ok(()) => Ok(()),
        Err(err) => Err(anyhow!(err.clone())),
    }
}

fn validate_metadata(
    metadata: &UpdateMetadata,
    signature: &UpdateSignature,
    options: &VerificationOptions<'_>,
) -> ResultType<()> {
    if metadata.schema_version != METADATA_SCHEMA_VERSION {
        return Err(anyhow!("unsupported update metadata schema version"));
    }
    if metadata.signature_key_id != signature.key_id {
        return Err(anyhow!("update metadata signature key id mismatch"));
    }
    if metadata.app != options.trust.app() {
        return Err(anyhow!("update metadata app mismatch"));
    }
    if metadata.package_id != options.expected_package_id {
        return Err(anyhow!("update metadata package id is not allowed"));
    }
    if options.requirements.expected_version != metadata.version {
        return Err(anyhow!("update metadata version mismatch"));
    }
    if options.requirements.expected_release_id != metadata.release_id {
        return Err(anyhow!("update metadata release id mismatch"));
    }
    for artifact in &metadata.artifacts {
        validate_artifact_file_name(&artifact.file_name)?;
    }
    Ok(())
}

fn select_artifact<'a>(
    metadata: &'a UpdateMetadata,
    query: &UpdateArtifactQuery<'_>,
) -> ResultType<&'a UpdateArtifact> {
    let mut matches = metadata.artifacts.iter().filter(|artifact| {
        artifact.platform == query.platform
            && artifact.arch == query.arch
            && artifact.format == query.format
    });
    let artifact = matches
        .next()
        .ok_or_else(|| anyhow!("matching update artifact not found"))?;
    if matches.next().is_some() {
        return Err(anyhow!("multiple matching update artifacts found"));
    }
    Ok(artifact)
}

#[cfg(target_os = "macos")]
#[cfg(test)]
#[path = "update_metadata_tests.rs"]
mod tests;
