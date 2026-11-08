//! Update channels (`stable` = `main`, `dev` = development builds).
//!
//! The feed is `appcast/<channel>.json` on the `update-feed` branch, Ed25519-signed with
//! the BetterDesk client update key. It is fetched over HTTPS only and verified before
//! anything is downloaded, so a compromised network cannot push a build.

use hbb_common::{anyhow::Context, config::LocalConfig, log, ResultType};
use serde_derive::Deserialize;
use std::time::{Duration, Instant};

use crate::update_metadata::{
    UpdateArtifactQuery, UpdateMetadataRequirements, VerifiedUpdateArtifact, PUBLIC_UPDATE_PACKAGE_ID,
};

/// LocalConfig key holding the selected channel.
pub const OPTION_UPDATE_CHANNEL: &str = "update-channel";

/// Raw feed location. Kept on a branch so the URL is stable across releases.
const FEED_BASE: &str =
    "https://raw.githubusercontent.com/UNITRONIX/BetterDesk-Client/update-feed/appcast";

/// Artifacts are published under GitHub release assets.
const RELEASE_BASE: &str = "https://github.com/UNITRONIX/BetterDesk-Client/releases/download";

const FEED_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const FEED_MAX_BYTES: u64 = 256 * 1024;
/// Do not hammer the feed: the check runs on a timer and from the settings tab.
const MIN_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 10);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateChannel {
    Stable,
    Dev,
}

impl UpdateChannel {
    pub fn as_str(&self) -> &'static str {
        match self {
            UpdateChannel::Stable => "stable",
            UpdateChannel::Dev => "dev",
        }
    }

    fn parse(value: &str) -> Option<UpdateChannel> {
        match value.trim().to_ascii_lowercase().as_str() {
            "stable" | "main" | "release" => Some(UpdateChannel::Stable),
            "dev" | "development" | "nightly" => Some(UpdateChannel::Dev),
            _ => None,
        }
    }

    /// Channel selected by the operator, defaulting to the build's own channel.
    ///
    /// Installs from `main` default to stable and installs from `dev` default to dev, so a
    /// tester never has to flip a switch and a production fleet never silently follows a
    /// development build.
    pub fn selected() -> UpdateChannel {
        if let Some(channel) = Self::parse(&LocalConfig::get_option(OPTION_UPDATE_CHANNEL)) {
            return channel;
        }
        Self::build_default()
    }

    fn build_default() -> UpdateChannel {
        if cfg!(debug_assertions) {
            UpdateChannel::Dev
        } else {
            UpdateChannel::Stable
        }
    }

    pub fn set_selected(&self) {
        LocalConfig::set_option(OPTION_UPDATE_CHANNEL.to_owned(), self.as_str().to_owned());
    }

    fn feed_url(&self) -> String {
        format!("{FEED_BASE}/{}.json", self.as_str())
    }

    fn signature_url(&self) -> String {
        format!("{FEED_BASE}/{}.json.sig", self.as_str())
    }
}

/// Result of a feed check, shown in Settings → Updates.
#[derive(Clone)]
pub struct FeedCheck {
    pub channel: UpdateChannel,
    pub current_version: String,
    pub available_version: Option<String>,
    pub release_id: Option<String>,
    pub artifact: Option<VerifiedUpdateArtifact>,
    pub error: Option<String>,
}

impl FeedCheck {
    pub fn has_update(&self) -> bool {
        self.artifact.is_some()
    }

    pub fn to_json(&self) -> String {
        let escape = |value: &str| {
            value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        };
        let optional = |value: &Option<String>| match value {
            Some(value) => format!("\"{}\"", escape(value)),
            None => "null".to_owned(),
        };
        format!(
            "{{\"channel\":\"{}\",\"current_version\":\"{}\",\"available_version\":{},\
             \"release_id\":{},\"has_update\":{},\"size\":{},\"sha256\":{},\"error\":{}}}",
            self.channel.as_str(),
            escape(&self.current_version),
            optional(&self.available_version),
            optional(&self.release_id),
            self.has_update(),
            self.artifact.as_ref().map(|a| a.size).unwrap_or(0),
            optional(&self.artifact.as_ref().map(|a| a.sha256.clone())),
            optional(&self.error),
        )
    }
}

#[derive(Deserialize)]
struct FeedEnvelope {
    schema_version: u32,
    app: String,
    channel: String,
    package_id: String,
    version: String,
    release_id: String,
    #[serde(rename = "published_at")]
    _published_at: String,
}

lazy_static::lazy_static! {
    static ref LAST_CHECK: std::sync::Mutex<Option<(Instant, FeedCheck)>> =
        std::sync::Mutex::new(None);
}

fn fetch_bytes(url: &str) -> ResultType<Vec<u8>> {
    // `*_strict` refuses to downgrade to plaintext or accept an invalid certificate:
    // the feed decides what gets installed, so TLS is not optional here.
    #[cfg(target_os = "macos")]
    let client = crate::hbbs_http::create_http_client_with_url_strict(url)?;
    #[cfg(not(target_os = "macos"))]
    let client = {
        let client = crate::hbbs_http::create_http_client_with_url(url);
        if !url.starts_with("https://") {
            hbb_common::bail!("update feed must be fetched over HTTPS: {url}");
        }
        client
    };
    let mut response = client.get(url).timeout(FEED_REQUEST_TIMEOUT).send()?;
    if !response.status().is_success() {
        hbb_common::bail!("failed to fetch {url}: {}", response.status());
    }
    let mut limited = std::io::Read::take(&mut response, FEED_MAX_BYTES + 1);
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut limited, &mut bytes)?;
    if bytes.len() as u64 > FEED_MAX_BYTES {
        hbb_common::bail!("update feed is larger than {FEED_MAX_BYTES} bytes");
    }
    Ok(bytes)
}

fn artifact_query() -> UpdateArtifactQuery<'static> {
    #[cfg(target_os = "windows")]
    let update_msi = crate::platform::is_msi_installed().unwrap_or(false) && !crate::is_custom_client();
    #[cfg(not(target_os = "windows"))]
    let update_msi = false;
    crate::updater::current_update_artifact_query(update_msi)
}

/// Check the selected channel for a newer build.
///
/// `force` bypasses the minimum interval; the UI passes `true` for an explicit "Check now".
pub fn check(force: bool) -> FeedCheck {
    let channel = UpdateChannel::selected();
    let current_version = crate::VERSION.to_owned();
    let mut result = FeedCheck {
        channel,
        current_version: current_version.clone(),
        available_version: None,
        release_id: None,
        artifact: None,
        error: None,
    };

    {
        let last = LAST_CHECK.lock().unwrap_or_else(|err| err.into_inner());
        if !force {
            if let Some((at, cached)) = last.as_ref() {
                if at.elapsed() < MIN_CHECK_INTERVAL {
                    return cached.clone();
                }
            }
        }
    }

    if let Err(err) = check_inner(&channel, &current_version, &mut result) {
        // A failed check is not fatal: keep the client usable and surface the reason in the
        // settings tab instead of retrying in a loop.
        log::info!("update check failed ({}): {err}", channel.as_str());
        result.error = Some(err.to_string());
    }

    *LAST_CHECK.lock().unwrap_or_else(|err| err.into_inner()) = Some((Instant::now(), result.clone()));
    result
}

fn check_inner(
    channel: &UpdateChannel,
    current_version: &str,
    result: &mut FeedCheck,
) -> ResultType<()> {
    let feed_bytes = fetch_bytes(&channel.feed_url()).context("fetch update feed")?;
    let signature_bytes = fetch_bytes(&channel.signature_url()).context("fetch feed signature")?;

    let envelope: FeedEnvelope =
        serde_json::from_slice(&feed_bytes).context("parse update feed")?;
    if envelope.schema_version != 1 {
        hbb_common::bail!("unsupported update feed schema {}", envelope.schema_version);
    }
    if envelope.app != "betterdesk-client" {
        hbb_common::bail!("update feed is for {}, not BetterDesk", envelope.app);
    }
    if envelope.channel != channel.as_str() {
        // A dev feed served where stable was requested is exactly the mistake this guards.
        hbb_common::bail!(
            "update feed channel mismatch: requested {}, got {}",
            channel.as_str(),
            envelope.channel
        );
    }
    if envelope.package_id != PUBLIC_UPDATE_PACKAGE_ID && envelope.package_id != "betterdesk-desktop" {
        hbb_common::bail!("update feed package id {} is not allowed", envelope.package_id);
    }

    result.available_version = Some(envelope.version.clone());
    result.release_id = Some(envelope.release_id.clone());

    if !is_newer(&envelope.version, current_version) {
        return Ok(());
    }

    let release_prefix = format!("{RELEASE_BASE}/{}/", envelope.release_id);
    let requirements = UpdateMetadataRequirements {
        expected_version: envelope.version.as_str(),
        expected_release_id: envelope.release_id.as_str(),
        expected_artifact_url_prefix: release_prefix.as_str(),
        artifact: artifact_query(),
        expected_package_id: envelope.package_id.as_str(),
    };
    let artifact = crate::update_metadata::verify_betterdesk_update_metadata(
        &feed_bytes,
        &signature_bytes,
        requirements,
    )?;
    result.artifact = Some(artifact);
    Ok(())
}

/// Compare `available` against `current` using the client's own version ordering.
fn is_newer(available: &str, current: &str) -> bool {
    let available_number = hbb_common::get_version_number(available);
    let current_number = hbb_common::get_version_number(current);
    if available_number > 0 && current_number > 0 {
        return available_number > current_number;
    }
    // Unparsable versions (a dev tag) fall back to string inequality so a dev build can
    // still be replaced; verification makes a wrong answer harmless.
    available != current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_parsing_accepts_operator_spellings() {
        assert_eq!(UpdateChannel::parse("stable"), Some(UpdateChannel::Stable));
        assert_eq!(UpdateChannel::parse("MAIN"), Some(UpdateChannel::Stable));
        assert_eq!(UpdateChannel::parse(" dev "), Some(UpdateChannel::Dev));
        assert_eq!(UpdateChannel::parse("nightly"), Some(UpdateChannel::Dev));
        assert_eq!(UpdateChannel::parse("beta"), None);
    }

    #[test]
    fn feed_urls_are_distinct_and_https() {
        assert!(UpdateChannel::Stable.feed_url().starts_with("https://"));
        assert!(UpdateChannel::Stable.feed_url().ends_with("/appcast/stable.json"));
        assert!(UpdateChannel::Dev.feed_url().ends_with("/appcast/dev.json"));
        assert!(UpdateChannel::Dev.signature_url().ends_with("dev.json.sig"));
    }

    #[test]
    fn newer_versions_compare_numerically() {
        assert!(is_newer("1.5.1", "1.5.0"));
        assert!(!is_newer("1.5.0", "1.5.0"));
        assert!(!is_newer("1.4.9", "1.5.0"));
    }

    #[test]
    fn check_json_surfaces_error_without_update() {
        let check = FeedCheck {
            channel: UpdateChannel::Dev,
            current_version: "1.5.0".to_owned(),
            available_version: None,
            release_id: None,
            artifact: None,
            error: Some("boom \"x\"".to_owned()),
        };
        let json = check.to_json();
        assert!(json.contains("\"has_update\":false"));
        assert!(json.contains("\"channel\":\"dev\""));
        assert!(json.contains("boom \\\"x\\\""));
    }
}
