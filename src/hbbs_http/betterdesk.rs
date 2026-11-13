//! BetterDesk-specific HTTP helpers for the official desktop client.
//!
//! Branding sync (`/api/branding`), device enrollment (`/api/devices/register`),
//! and health/server-key probes. Base URL comes from configured `api-server`.

use std::{
    fs,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

use hbb_common::{
    base64::Engine as _,
    bail, config,
    config::{keys, Config, LocalConfig},
    log, ResultType,
};
use serde::Deserialize;
use serde_json::Value;

use super::create_http_client_async_with_url;

/// Product marker sent as `device_type` / `client_product` (legacy CDAP-safe).
pub const CLIENT_PRODUCT: &str = config::BETTERDESK_CLIENT_PRODUCT;

/// Generator SKU: full desktop client (outbound + inbound).
pub const PRODUCT_SKU_DESKTOP: &str = "betterdesk-desktop";
/// Generator SKU: Support Agent (hard `conn-type: incoming`).
pub const PRODUCT_SKU_SUPPORT: &str = "betterdesk-support";

pub const CONN_MODE_NORMAL: &str = "normal";
pub const CONN_MODE_INCOMING_ONLY: &str = "incoming-only";

const OPTION_BRANDING_SOURCE: &str = "branding-source";
const OPTION_BRANDING_REVISION: &str = "branding-revision";
const OPTION_BRANDING_ACCENT: &str = "branding-accent-color";
const OPTION_BRANDING_BACKGROUND: &str = "branding-background-color";
const OPTION_BRANDING_LOGO_PATH: &str = "branding-logo-path";
const OPTION_BRANDING_SYNCED_API: &str = "branding-synced-api";
const BRANDING_SOURCE_SERVER: &str = "server";
const BRANDING_LOGO_MAX_BYTES: usize = 512 * 1024;
const BRANDING_POLL_INTERVAL: Duration = Duration::from_secs(60);
const TELEMETRY_KEY_POLL_INTERVAL: Duration = Duration::from_secs(3600);
const OPTION_ENROLLMENT_STATUS: &str = "betterdesk-enrollment-status";
const OPTION_ENROLLMENT_LAST_ATTEMPT: &str = "betterdesk-enrollment-last-attempt";
/// Last human-readable server message (refusal reason, suggested device id).
const OPTION_ENROLLMENT_MESSAGE: &str = "betterdesk-enrollment-message";
/// Single-use enrollment token pasted by the operator (or baked into a bundle).
/// Sent on registration so a managed/locked server admits this device without a
/// manual Approve click.
const OPTION_ENROLLMENT_TOKEN: &str = "betterdesk-enrollment-token";
/// Device-bound credential issued by the server once the device is admitted.
///
/// It is what later proves this device's identity: the server only re-issues a
/// token, accepts a self-service TOTP reset, or serves device-scoped help when
/// the caller presents it (`hasBoundActiveDeviceToken`). Older builds dropped
/// the value from the enrollment response, so a reinstall or a token rotation
/// left the device unable to prove anything and requiring operator action.
const OPTION_DEVICE_TOKEN: &str = "betterdesk-device-token";
const ENROLLMENT_RETRY_SECS: u64 = 120;

/// Applied SKU from bake-in (`conn-type: incoming` → Support Agent).
#[inline]
pub fn product_sku() -> &'static str {
    if config::is_permanent_incoming_only() {
        PRODUCT_SKU_SUPPORT
    } else {
        PRODUCT_SKU_DESKTOP
    }
}

/// Applied connection mode from `HARD_SETTINGS` (not a server wish).
#[inline]
pub fn conn_mode() -> &'static str {
    match config::connection_mode() {
        "incoming-only" => CONN_MODE_INCOMING_ONLY,
        _ => CONN_MODE_NORMAL,
    }
}

pub fn connection_policy_fields() -> Value {
    let mut out = serde_json::json!({
        "effective_conn_mode": conn_mode(),
        "policy_revision": LocalConfig::get_connection_mode_revision(),
        "policy_status": LocalConfig::get_connection_mode_status(),
    });
    let command_id = LocalConfig::get_connection_mode_command_id();
    if !command_id.is_empty() {
        out["last_command_id"] = Value::String(command_id);
    }
    let error = LocalConfig::get_connection_mode_error();
    if !error.is_empty() {
        out["policy_error"] = Value::String(error);
    }
    out
}

pub fn device_capabilities() -> Vec<&'static str> {
    let mut capabilities = vec![
        "remote_desktop",
        "telemetry.metrics",
        "inventory.hardware",
        "telemetry.services",
        "telemetry.processes",
        "telemetry.events",
        "files.browse",
        "files.read",
        "file_transfer",
        "chat",
        "clipboard",
        "audio",
        "terminal",
        "service.control",
        "process.terminate",
        "restart",
    ];
    if config::is_remote_connection_mode_controlled() {
        capabilities.push("connection-mode.remote-control");
    }
    // Incoming-only restricts the connection direction, not the features
    // available after an approved operator session.
    capabilities
}

/// Fields for register / sysinfo / heartbeat so the panel can show device details.
pub fn device_identity_fields() -> Value {
    let enrollment_status = LocalConfig::get_option(OPTION_ENROLLMENT_STATUS);
    let branding_revision = LocalConfig::get_option(OPTION_BRANDING_REVISION);
    let branding_source = LocalConfig::get_option(OPTION_BRANDING_SOURCE);
    let mut out = serde_json::json!({
        "product_sku": product_sku(),
        "conn_mode": conn_mode(),
        "app_name": crate::get_app_name(),
        "capabilities": device_capabilities(),
    });
    if config::is_remote_connection_mode_controlled() {
        if let Some(object) = connection_policy_fields().as_object() {
            for (key, value) in object {
                out[key] = value.clone();
            }
        }
    }
    if !enrollment_status.is_empty() {
        out["enrollment_status"] = Value::String(enrollment_status);
    }
    if !branding_revision.is_empty() {
        out["branding_revision"] = Value::String(branding_revision);
    }
    if !branding_source.is_empty() {
        out["branding_source"] = Value::String(branding_source);
    }
    out
}

/// Merge [`device_identity_fields`] into an existing JSON object.
pub fn merge_device_identity(target: &mut Value) {
    if let (Some(obj), Some(id)) = (target.as_object_mut(), device_identity_fields().as_object()) {
        for (k, v) in id {
            obj.insert(k.clone(), v.clone());
        }
    }
}

lazy_static::lazy_static! {
    static ref LAST_BRANDING_POLL: Mutex<Option<Instant>> = Mutex::new(None);
    static ref TELEMETRY_SERVER_KEY: Mutex<Option<(String, String, String, Instant)>> = Mutex::new(None);
}

/// The API base used by background sync (branding, enrollment, telemetry).
///
/// Prefers a resolved HTTPS base, then an explicit `api-server`, then the legacy
/// derivation. Keeps working setups untouched: an operator who set `api-server` explicitly
/// keeps exactly that string.
fn api_base() -> String {
    if let Some(base) = super::api_base::cached_base() {
        return base.trim_end_matches('/').to_owned();
    }
    let api = crate::get_api_server(
        Config::get_option("api-server"),
        Config::get_option("custom-rendezvous-server"),
    );
    api.trim_end_matches('/').to_owned()
}

/// GET `{api}/api/health` — returns Ok(json) when API is reachable.
#[allow(dead_code)]
pub async fn fetch_health() -> ResultType<Value> {
    let base = api_base();
    if base.is_empty() {
        bail!("BetterDesk API server is not configured");
    }
    let url = format!("{base}/api/health");
    let client = create_http_client_async_with_url(&url).await;
    let resp = client.get(&url).send().await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("health check failed: HTTP {status} {text}");
    }
    match serde_json::from_str(&text) {
        Ok(v) => Ok(v),
        Err(_) => Ok(Value::String(text)),
    }
}

/// GET `{api}/api/branding` — optional white-label payload from the panel.
pub async fn fetch_branding() -> ResultType<Value> {
    let base = api_base();
    if base.is_empty() {
        bail!("BetterDesk API server is not configured");
    }
    let url = format!("{base}/api/branding");
    let client = create_http_client_async_with_url(&url).await;
    let revision = LocalConfig::get_option(OPTION_BRANDING_REVISION);
    let mut request = client.get(&url);
    if !revision.is_empty() && revision != "0" {
        request = request.header(reqwest::header::IF_NONE_MATCH, format!("\"{revision}\""));
    }
    let resp = request.send().await?;
    let status = resp.status();
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(serde_json::json!({ "revision": revision }));
    }
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("branding fetch failed: HTTP {status}");
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

/// GET `{api}/api/server-key` — public key helpers for Network auto-fill UX.
#[allow(dead_code)]
pub async fn fetch_server_key() -> ResultType<String> {
    let base = api_base();
    if base.is_empty() {
        bail!("BetterDesk API server is not configured");
    }
    let url = format!("{base}/api/server-key");
    let client = create_http_client_async_with_url(&url).await;
    let resp = client.get(&url).send().await?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        bail!("server-key fetch failed: HTTP {status}");
    }
    if let Ok(v) = serde_json::from_str::<Value>(&text) {
        if let Some(k) = v.get("key").and_then(|x| x.as_str()) {
            return Ok(k.to_owned());
        }
    }
    Ok(text.trim().to_owned())
}

/// GET `{api}/api/telemetry/key` — public X25519 key used for HTTP payload
/// protection. The key is cached briefly so heartbeat does not add a request
/// for every sample.
pub async fn fetch_telemetry_server_key() -> ResultType<(String, String)> {
    {
        let cache = TELEMETRY_SERVER_KEY.lock().unwrap();
        if let Some((key_id, public_key, _signature, fetched_at)) = cache.as_ref() {
            if fetched_at.elapsed() < TELEMETRY_KEY_POLL_INTERVAL {
                return Ok((key_id.clone(), public_key.clone()));
            }
        }
    }

    let base = api_base();
    if base.is_empty() {
        bail!("BetterDesk API server is not configured");
    }
    let url = format!("{base}/api/telemetry/key");
    let client = create_http_client_async_with_url(&url).await;
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        bail!("telemetry key fetch failed: HTTP {}", resp.status());
    }
    let value: Value = resp.json().await?;
    let key_id = match value
        .get("key_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value.to_owned(),
        None => bail!("telemetry key id missing"),
    };
    let public_key = match value
        .get("public_key")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value.to_owned(),
        None => bail!("telemetry public key missing"),
    };
    let signature = match value
        .get("signature")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        Some(value) => value.to_owned(),
        None => bail!("telemetry key signature missing"),
    };
    let configured_key = Config::get_option(keys::OPTION_KEY);
    if configured_key.is_empty() {
        bail!("BetterDesk server key is not configured");
    }
    let configured_key = hbb_common::base64::engine::general_purpose::STANDARD
        .decode(configured_key)
        .map_err(|_| hbb_common::anyhow::anyhow!("invalid BetterDesk server key"))?;
    let signing_key = hbb_common::sodiumoxide::crypto::sign::PublicKey::from_slice(&configured_key)
        .ok_or_else(|| hbb_common::anyhow::anyhow!("invalid BetterDesk server key size"))?;
    let signed = hbb_common::base64::engine::general_purpose::STANDARD
        .decode(&signature)
        .map_err(|_| hbb_common::anyhow::anyhow!("invalid telemetry key signature"))?;
    let verified = hbb_common::sodiumoxide::crypto::sign::verify(&signed, &signing_key)
        .map_err(|_| hbb_common::anyhow::anyhow!("telemetry key signature mismatch"))?;
    let expected = format!("1|{}|{}", key_id, public_key);
    if verified != expected.as_bytes() {
        bail!("telemetry key identity mismatch");
    }
    let mut cache = TELEMETRY_SERVER_KEY.lock().unwrap();
    *cache = Some((key_id.clone(), public_key.clone(), signature, Instant::now()));
    LocalConfig::set_option(
        "betterdesk-telemetry-server-key-id".to_owned(),
        key_id.clone(),
    );
    Ok((key_id, public_key))
}

/// Log a one-line identity banner at startup (no network).
pub fn log_client_identity() {
    log::info!(
        "BetterDesk official client product={} sku={} conn_mode={} app={}",
        CLIENT_PRODUCT,
        product_sku(),
        conn_mode(),
        crate::get_app_name()
    );
}

lazy_static::lazy_static! {
    static ref LAST_ENROLLMENT_POLL: Mutex<Option<Instant>> = Mutex::new(None);
}

/// Enrollment token handling (2026-11-13).
///
/// The server admits a new device through operator approval **or** a single-use
/// enrollment token. Stock clients always relied on the approval path, but a
/// managed/locked server cannot know about a device that never showed up in the
/// Registrations queue — and an unattended install cannot click Approve. Sending
/// the token lets such a device enrol by itself.
///
/// The token is single-use per device on the server, so it is cleared locally
/// once the device reaches `approved`; keeping it would only make replay
/// attempts (and confusing diagnostics) possible.
pub fn set_enrollment_token(token: &str) {
    let cleaned = token.trim();
    if cleaned.is_empty() {
        LocalConfig::set_option(OPTION_ENROLLMENT_TOKEN.to_owned(), String::new());
        reset_enrollment_backoff();
        return;
    }
    LocalConfig::set_option(OPTION_ENROLLMENT_TOKEN.to_owned(), cleaned.to_owned());
    // A new token means the previous outcome no longer describes reality.
    LocalConfig::set_option(OPTION_ENROLLMENT_STATUS.to_owned(), String::new());
    LocalConfig::set_option(OPTION_ENROLLMENT_MESSAGE.to_owned(), String::new());
    reset_enrollment_backoff();
}

/// Clear the retry timer so the next sync attempt runs immediately.
///
/// Without this, pasting a token (or an operator approving the device) would
/// still wait out the current interval — up to an hour for a terminal state.
fn reset_enrollment_backoff() {
    *LAST_ENROLLMENT_POLL.lock().unwrap() = None;
}

/// The admission token currently in effect.
///
/// `LocalConfig` holds the durable copy (device-local file, never part of a
/// server-pushed option set). A running daemon receives the value over IPC,
/// which lands in `Config`, so that copy is read as a fallback: the CLI may set
/// the token while the service is already running.
pub fn enrollment_token() -> String {
    let local = LocalConfig::get_option(OPTION_ENROLLMENT_TOKEN);
    if !local.trim().is_empty() {
        return local.trim().to_owned();
    }
    Config::get_option(OPTION_ENROLLMENT_TOKEN).trim().to_owned()
}

/// Human-readable enrolment state for the UI/diagnostics card.
pub fn enrollment_state() -> &'static str {
    match LocalConfig::get_option(OPTION_ENROLLMENT_STATUS).as_str() {
        "approved" => "approved",
        "pending" => "pending",
        "rejected" => "rejected",
        "banned" => "banned",
        "removed" => "removed",
        "identity-conflict" => "identity-conflict",
        "awaiting-token-or-approval" => "awaiting-token-or-approval",
        "token-rejected" => "token-rejected",
        _ if !enrollment_token().is_empty() => "token-set",
        _ => "unregistered",
    }
}

/// Classify an enrolment refusal into a locally-stored state.
///
/// Two server generations answer differently: the Go server sends
/// `status: "rejected"` with a human message ("Device is banned", "Device has
/// been removed", "Enrollment is locked — a valid token is required"), while the
/// Node panel-side endpoint used machine codes in `error`
/// (`enrollment_proof_required`). Both are mapped here, and the message is
/// keyword-matched, so diagnostics can distinguish "waiting for an operator"
/// from "this device will never be admitted".
///
/// Returns `None` when the answer is not a refusal.
fn classify_refusal(
    http_status: u16,
    error_code: Option<&str>,
    message: &str,
    token_present: bool,
) -> Option<&'static str> {
    let refused = matches!(http_status, 401 | 403) || error_code.is_some();
    if !refused {
        return None;
    }
    let admission = || {
        if token_present {
            "token-rejected"
        } else {
            "awaiting-token-or-approval"
        }
    };
    let lowered = message.to_ascii_lowercase();
    Some(match error_code {
        Some("device_banned") => "banned",
        Some("device_removed") => "removed",
        Some("enrollment_proof_required") => admission(),
        _ if lowered.contains("banned") => "banned",
        _ if lowered.contains("removed") => "removed",
        _ if lowered.contains("locked") => admission(),
        _ => "rejected",
    })
}

/// Enrolment retry pause for states an operator must resolve.
///
/// Retrying a pending device every two minutes is right: approval may arrive at
/// any moment. Retrying a *banned*, *removed*, or *identity-conflicting* device
/// is not — no retry can succeed until an operator acts, and the server logs the
/// refusal each time. Back off to an hour so the device still recovers on its own
/// after an operator fixes it, without flooding the server meanwhile.
const ENROLLMENT_TERMINAL_RETRY_SECS: u64 = 3600;

/// States that only an operator action (or a new identity) can leave.
fn is_operator_action_state(state: &str) -> bool {
    matches!(
        state,
        "banned" | "removed" | "identity-conflict" | "token-rejected"
    )
}

/// Device-bound credential issued at enrollment (empty before that).
pub fn device_token() -> String {
    LocalConfig::get_option(OPTION_DEVICE_TOKEN).trim().to_owned()
}

fn store_device_token(token: &str) {
    let token = token.trim();
    if token.is_empty() || token == device_token() {
        return;
    }
    LocalConfig::set_option(OPTION_DEVICE_TOKEN.to_owned(), token.to_owned());
    log::info!("BetterDesk device credential stored (len={})", token.len());
}

/// Last server message for the current state (empty when there is none).
pub fn enrollment_message() -> String {
    LocalConfig::get_option(OPTION_ENROLLMENT_MESSAGE)
}

fn clear_enrollment_token() {
    if !enrollment_token().is_empty() {
        LocalConfig::set_option(OPTION_ENROLLMENT_TOKEN.to_owned(), String::new());
        Config::set_option(OPTION_ENROLLMENT_TOKEN.to_owned(), String::new());
    }
}

/// Push the token into the running service over IPC.
///
/// The CLI usually talks to a separate daemon process (installed service or the
/// already-running tray app). Writing the config alone leaves that process on
/// its old value until it restarts, so the enrolment retry could keep reporting
/// "awaiting token" for hours after the operator set one.
///
/// Uses `ipc::set_option`, which reads the daemon's current option map first, so
/// unrelated settings are preserved. Failures are expected when no daemon runs
/// (portable/CLI-only use) and are only logged.
pub fn push_enrollment_token_to_daemon(token: &str) {
    let value = token.trim().to_owned();
    // `ipc::set_option` builds its own Tokio runtime internally, which panics
    // when an ambient runtime exists (Flutter FFI entry points can have one).
    // A short-lived OS thread always starts with a clean runtime context.
    let handle = std::thread::Builder::new()
        .name("enrollment-token-ipc".to_owned())
        .spawn(move || {
            if value.is_empty() {
                crate::ipc::set_option(OPTION_ENROLLMENT_TOKEN, "");
            } else {
                crate::ipc::set_option(OPTION_ENROLLMENT_TOKEN, &value);
            }
        });
    match handle {
        Ok(handle) => {
            let _ = handle.join();
            log::debug!("enrollment token pushed to daemon");
        }
        Err(err) => log::debug!("enrollment token IPC push skipped: {err}"),
    }
}

/// POST `{api}/api/devices/register` for BetterDesk desktop / Support (incoming-only).
///
/// Uses `device_type: betterdesk-desktop` so the server does **not** apply the
/// legacy CDAP Support Agent proof path. Managed mode → pending queue; open → approved.
pub async fn sync_device_enrollment() {
    let base = api_base();
    if base.is_empty() || crate::is_public(&base) {
        return;
    }

    let status = LocalConfig::get_option(OPTION_ENROLLMENT_STATUS);
    if status == "approved" {
        return;
    }

    {
        let mut last = LAST_ENROLLMENT_POLL.lock().unwrap();
        if let Some(t) = *last {
            // States in `is_operator_action_state` cannot change on their own, so
            // they get a long pause instead of hammering the server every two
            // minutes with a request that will be refused again.
            let interval = if is_operator_action_state(&status) {
                ENROLLMENT_TERMINAL_RETRY_SECS
            } else {
                ENROLLMENT_RETRY_SECS
            };
            if t.elapsed() < Duration::from_secs(interval) {
                return;
            }
        }
        *last = Some(Instant::now());
    }

    let device_id = Config::get_id();
    if device_id.is_empty() {
        return;
    }
    let device_uuid = String::from_utf8_lossy(&hbb_common::get_uuid()).into_owned();

    let mut body = serde_json::json!({
        "device_id": device_id,
        "uuid": device_uuid,
        "hostname": crate::hostname(),
        "platform": std::env::consts::OS,
        "version": crate::VERSION,
        // Keep legacy device_type so the server does not take the old CDAP proof path.
        "device_type": CLIENT_PRODUCT,
        "tags": if config::is_permanent_incoming_only() {
            "betterdesk-support,incoming-only"
        } else if config::is_incoming_only() {
            "betterdesk-desktop,incoming-only"
        } else {
            "betterdesk-desktop"
        },
    });
    // Admission / identity credential: without one a managed or locked server
    // only offers the manual Approve path, and an unattended install would never
    // appear online.
    //
    // Two credentials exist and they are not interchangeable in time: the
    // single-use enrollment token admits a *new* device, and the device token the
    // server issues on approval proves this device afterwards. The device token
    // wins once stored — it is bound to the device id, so it keeps working for
    // refreshes and for re-issuing a rotated credential.
    //
    // The server reads the value as `token` (`EnrollmentRequest.Token` in
    // betterdesk-server/api/branding_handlers.go) or as a bearer credential.
    // Sending an invented field name is silently ignored, which is exactly the
    // failure mode that makes token enrolment look broken, so both spellings are
    // sent: `token` for current servers, `enrollment_token` for older
    // panel-side implementations that used that name.
    let device_credential = device_token();
    let admission_token = enrollment_token();
    let credential = if device_credential.is_empty() {
        admission_token
    } else {
        device_credential
    };
    if !credential.is_empty() {
        body["token"] = Value::String(credential.clone());
        body["enrollment_token"] = Value::String(credential.clone());
    }
    merge_device_identity(&mut body);

    let url = format!("{base}/api/devices/register");
    let client = create_http_client_async_with_url(&url).await;
    let mut request = client.post(&url).json(&body);
    if !credential.is_empty() {
        request = request.header(reqwest::header::AUTHORIZATION, format!("Bearer {credential}"));
    }
    let resp = match request.send().await {
        Ok(r) => r,
        Err(err) => {
            log::debug!("enrollment register skipped: {err}");
            return;
        }
    };
    let status_code = resp.status();
    let text = resp.text().await.unwrap_or_default();
    // Accept every documented answer: 200/202 (approved/pending), 403 (refused),
    // 404/409 (unknown device, identity conflict). Anything else is a transport
    // or proxy problem and is not worth a state change.
    if !(status_code.is_success()
        || matches!(status_code.as_u16(), 401 | 403 | 404 | 409))
    {
        log::debug!("enrollment register HTTP {status_code}: {text}");
        return;
    }

    let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let message = value.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let reason = value
        .get("error")
        .and_then(|v| v.as_str())
        .filter(|value| !value.is_empty());
    let token_present = !enrollment_token().is_empty() || !device_token().is_empty();

    // 409 identity_conflict: another machine already owns this device ID. The
    // server suggests a free one; record it so the operator can act instead of
    // watching an endless retry loop. Checked before the refusal mapping because
    // this answer also carries an `error` code.
    if status_code.as_u16() == 409 || reason == Some("identity_conflict") {
        let suggested = value
            .get("suggested_device_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        log::warn!("BetterDesk enrollment identity conflict; suggested id={suggested}");
        LocalConfig::set_option(
            OPTION_ENROLLMENT_STATUS.to_owned(),
            "identity-conflict".to_owned(),
        );
        LocalConfig::set_option(
            OPTION_ENROLLMENT_LAST_ATTEMPT.to_owned(),
            format!("{}", hbb_common::get_time()),
        );
        let note = if suggested.is_empty() {
            message.to_owned()
        } else {
            format!("suggested_device_id={suggested}")
        };
        if !note.is_empty() {
            LocalConfig::set_option(OPTION_ENROLLMENT_MESSAGE.to_owned(), note);
        }
        return;
    }

    if let Some(mapped) = classify_refusal(status_code.as_u16(), reason, message, token_present) {
        log::warn!(
            "BetterDesk enrollment refused ({status_code}): {} {message}",
            reason.unwrap_or("rejected")
        );
        // A rejected token is kept on purpose: the operator pasted it, and
        // dropping it would erase the only evidence of what was tried. Retrying
        // it is harmless — the server never consumes a token that did not admit
        // the device.
        LocalConfig::set_option(OPTION_ENROLLMENT_STATUS.to_owned(), mapped.to_owned());
        LocalConfig::set_option(
            OPTION_ENROLLMENT_LAST_ATTEMPT.to_owned(),
            format!("{}", hbb_common::get_time()),
        );
        if !message.is_empty() {
            LocalConfig::set_option(OPTION_ENROLLMENT_MESSAGE.to_owned(), message.to_owned());
        }
        return;
    }

    let enroll_status = value
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    if !enroll_status.is_empty() {
        LocalConfig::set_option(OPTION_ENROLLMENT_STATUS.to_owned(), enroll_status.clone());
        LocalConfig::set_option(
            OPTION_ENROLLMENT_LAST_ATTEMPT.to_owned(),
            format!("{}", hbb_common::get_time()),
        );
        log::info!("BetterDesk enrollment status={enroll_status}");
    }

    // Persist the device-bound credential the server issues on approval. It is
    // the only way this install can later prove its identity (token refresh,
    // device self-service), and older builds silently discarded it.
    if let Some(issued) = value.get("device_token").and_then(|v| v.as_str()) {
        store_device_token(issued);
    }

    // Poll status when pending
    if enroll_status == "pending" {
        let status_url = format!("{base}/api/devices/register/status?device_id={device_id}");
        if let Ok(r) = client.get(&status_url).send().await {
            if let Ok(t) = r.text().await {
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    if let Some(s) = v.get("status").and_then(|x| x.as_str()) {
                        LocalConfig::set_option(OPTION_ENROLLMENT_STATUS.to_owned(), s.to_owned());
                    }
                    if let Some(issued) = v.get("device_token").and_then(|x| x.as_str()) {
                        store_device_token(issued);
                    }
                }
            }
        }
    }

    // The token is single-use on the server. Once the device is approved it can
    // never be needed again, so drop it instead of retrying with a spent token.
    let final_status = LocalConfig::get_option(OPTION_ENROLLMENT_STATUS);
    if final_status == "approved" || enroll_status == "approved" {
        clear_enrollment_token();
        log::info!("BetterDesk enrollment approved; enrollment token cleared");
    }
}

#[derive(Debug, Default, Deserialize)]
struct BrandingLogoPayload {
    #[serde(default)]
    mime: String,
    #[serde(default)]
    data_base64: String,
    #[serde(default)]
    url: String,
}

#[derive(Debug, Default, Deserialize)]
struct BrandingPayload {
    #[serde(default)]
    revision: String,
    #[serde(default)]
    company_name: String,
    #[serde(default)]
    phone: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    website: String,
    #[serde(default)]
    accent_color: String,
    #[serde(default)]
    background_color: String,
    #[serde(default)]
    support_contact: String,
    #[serde(default)]
    logo: Option<BrandingLogoPayload>,
}

fn is_server_managed() -> bool {
    LocalConfig::get_option(OPTION_BRANDING_SOURCE) == BRANDING_SOURCE_SERVER
}

fn notify_branding_updated() {
    #[cfg(feature = "flutter")]
    {
        let event = serde_json::json!({
            "name": "client_branding",
            "action": "updated",
        });
        let _ = crate::flutter::push_global_event(
            crate::flutter::APP_TYPE_MAIN,
            event.to_string(),
        );
    }
}

fn clear_server_logo_files() {
    let path = LocalConfig::get_option(OPTION_BRANDING_LOGO_PATH);
    if !path.is_empty() {
        let _ = fs::remove_file(&path);
    }
    // Best-effort cleanup of previous server logo variants in config dir.
    let probe = Config::path("betterdesk_branding_logo_server_probe");
    if let Some(dir) = probe.parent() {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("betterdesk_branding_logo_server") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }
}

/// Clear branding previously applied from the BetterDesk API (not manual local branding).
pub fn clear_server_branding() {
    if !is_server_managed() {
        LocalConfig::set_option(OPTION_BRANDING_SYNCED_API.to_owned(), "".to_owned());
        LocalConfig::set_option(OPTION_BRANDING_REVISION.to_owned(), "".to_owned());
        return;
    }
    clear_server_logo_files();
    LocalConfig::set_option(keys::OPTION_BRANDING_COMPANY_NAME.to_owned(), "".to_owned());
    LocalConfig::set_option(keys::OPTION_BRANDING_PHONE.to_owned(), "".to_owned());
    LocalConfig::set_option(keys::OPTION_BRANDING_EMAIL.to_owned(), "".to_owned());
    LocalConfig::set_option(keys::OPTION_BRANDING_WEBSITE.to_owned(), "".to_owned());
    LocalConfig::set_option(keys::OPTION_BRANDING_LOGO.to_owned(), "".to_owned());
    LocalConfig::set_option(OPTION_BRANDING_LOGO_PATH.to_owned(), "".to_owned());
    LocalConfig::set_option(OPTION_BRANDING_ACCENT.to_owned(), "".to_owned());
    LocalConfig::set_option(OPTION_BRANDING_BACKGROUND.to_owned(), "".to_owned());
    LocalConfig::set_option(OPTION_BRANDING_SOURCE.to_owned(), "".to_owned());
    LocalConfig::set_option(OPTION_BRANDING_REVISION.to_owned(), "".to_owned());
    LocalConfig::set_option(OPTION_BRANDING_SYNCED_API.to_owned(), "".to_owned());
    notify_branding_updated();
    log::info!("cleared server-managed BetterDesk branding");
}

fn logo_ext_from_mime(mime: &str) -> &'static str {
    match mime.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/webp" => "webp",
        _ => "png",
    }
}

fn write_logo_payload(logo: &BrandingLogoPayload) -> ResultType<PathBuf> {
    if logo.data_base64.is_empty() {
        bail!("logo payload empty");
    }
    let mime = logo.mime.to_ascii_lowercase();
    if !matches!(
        mime.as_str(),
        "image/png" | "image/jpeg" | "image/jpg" | "image/webp"
    ) {
        bail!("unsupported logo mime");
    }
    let raw = crate::decode64(logo.data_base64.trim())
        .map_err(|_| hbb_common::anyhow::anyhow!("invalid logo base64"))?;
    if raw.is_empty() || raw.len() > BRANDING_LOGO_MAX_BYTES {
        bail!("logo size out of range");
    }
    clear_server_logo_files();
    let ext = logo_ext_from_mime(&mime);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = Config::path(format!("betterdesk_branding_logo_server_{stamp}.{ext}"));
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, raw)?;
    Ok(path)
}

async fn download_logo_url(url: &str) -> ResultType<PathBuf> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        bail!("logo url must be http(s)");
    }
    let client = create_http_client_async_with_url(url).await;
    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        bail!("logo download failed: HTTP {}", resp.status());
    }
    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/png")
        .split(';')
        .next()
        .unwrap_or("image/png")
        .trim()
        .to_owned();
    let bytes = resp.bytes().await?;
    if bytes.is_empty() || bytes.len() > BRANDING_LOGO_MAX_BYTES {
        bail!("logo size out of range");
    }
    let payload = BrandingLogoPayload {
        mime,
        data_base64: crate::encode64(&bytes),
        url: String::new(),
    };
    write_logo_payload(&payload)
}

fn apply_text_fields(payload: &BrandingPayload) {
    let company = payload.company_name.trim();
    // Prefer explicit website; fall back to support_contact when it looks like a URL.
    let website = {
        let w = payload.website.trim();
        if !w.is_empty() {
            w.to_owned()
        } else {
            let s = payload.support_contact.trim();
            if s.starts_with("http://") || s.starts_with("https://") {
                s.to_owned()
            } else {
                String::new()
            }
        }
    };
    LocalConfig::set_option(
        keys::OPTION_BRANDING_COMPANY_NAME.to_owned(),
        company.to_owned(),
    );
    LocalConfig::set_option(
        keys::OPTION_BRANDING_PHONE.to_owned(),
        payload.phone.trim().to_owned(),
    );
    LocalConfig::set_option(
        keys::OPTION_BRANDING_EMAIL.to_owned(),
        payload.email.trim().to_owned(),
    );
    LocalConfig::set_option(keys::OPTION_BRANDING_WEBSITE.to_owned(), website);
    LocalConfig::set_option(
        OPTION_BRANDING_ACCENT.to_owned(),
        payload.accent_color.trim().to_owned(),
    );
    LocalConfig::set_option(
        OPTION_BRANDING_BACKGROUND.to_owned(),
        payload.background_color.trim().to_owned(),
    );
}

async fn apply_branding_payload(base: &str, payload: BrandingPayload) -> ResultType<()> {
    let revision = payload.revision.trim();
    // revision "0"/empty means operator never saved Client Branding — do not lock UI.
    if revision.is_empty() || revision == "0" {
        if is_server_managed() {
            clear_server_branding();
        }
        LocalConfig::set_option(OPTION_BRANDING_SYNCED_API.to_owned(), base.to_owned());
        LocalConfig::set_option(OPTION_BRANDING_REVISION.to_owned(), "0".to_owned());
        return Ok(());
    }

    let empty_profile = payload.company_name.trim().is_empty()
        && payload.phone.trim().is_empty()
        && payload.email.trim().is_empty()
        && payload.website.trim().is_empty()
        && payload.accent_color.trim().is_empty()
        && payload.background_color.trim().is_empty()
        && payload
            .logo
            .as_ref()
            .map(|l| l.data_base64.is_empty() && l.url.is_empty())
            .unwrap_or(true);
    if empty_profile {
        if is_server_managed() {
            clear_server_branding();
        }
        LocalConfig::set_option(OPTION_BRANDING_SYNCED_API.to_owned(), base.to_owned());
        LocalConfig::set_option(OPTION_BRANDING_REVISION.to_owned(), revision.to_owned());
        return Ok(());
    }

    apply_text_fields(&payload);

    let mut has_logo = false;
    if let Some(logo) = payload.logo.as_ref() {
        if !logo.data_base64.is_empty() {
            match write_logo_payload(logo) {
                Ok(path) => {
                    LocalConfig::set_option(
                        OPTION_BRANDING_LOGO_PATH.to_owned(),
                        path.to_string_lossy().to_string(),
                    );
                    LocalConfig::set_option(keys::OPTION_BRANDING_LOGO.to_owned(), "Y".to_owned());
                    has_logo = true;
                }
                Err(err) => log::warn!("branding logo write failed: {err}"),
            }
        } else if !logo.url.is_empty() {
            match download_logo_url(&logo.url).await {
                Ok(path) => {
                    LocalConfig::set_option(
                        OPTION_BRANDING_LOGO_PATH.to_owned(),
                        path.to_string_lossy().to_string(),
                    );
                    LocalConfig::set_option(keys::OPTION_BRANDING_LOGO.to_owned(), "Y".to_owned());
                    has_logo = true;
                }
                Err(err) => log::warn!("branding logo download failed: {err}"),
            }
        }
    }
    if !has_logo {
        clear_server_logo_files();
        LocalConfig::set_option(OPTION_BRANDING_LOGO_PATH.to_owned(), "".to_owned());
        LocalConfig::set_option(keys::OPTION_BRANDING_LOGO.to_owned(), "".to_owned());
    }

    LocalConfig::set_option(
        OPTION_BRANDING_SOURCE.to_owned(),
        BRANDING_SOURCE_SERVER.to_owned(),
    );
    LocalConfig::set_option(
        OPTION_BRANDING_REVISION.to_owned(),
        payload.revision.clone(),
    );
    LocalConfig::set_option(OPTION_BRANDING_SYNCED_API.to_owned(), base.to_owned());
    notify_branding_updated();
    log::info!(
        "applied BetterDesk server branding revision={}",
        payload.revision
    );
    Ok(())
}

/// Called from the hbbs sync loop: fetch and apply Client Branding when API is configured.
pub async fn sync_client_branding() {
    let base = api_base();
    if base.is_empty() || crate::is_public(&base) {
        if is_server_managed() {
            clear_server_branding();
        } else {
            LocalConfig::set_option(OPTION_BRANDING_SYNCED_API.to_owned(), "".to_owned());
            LocalConfig::set_option(OPTION_BRANDING_REVISION.to_owned(), "".to_owned());
        }
        return;
    }

    let synced = LocalConfig::get_option(OPTION_BRANDING_SYNCED_API);
    if !synced.is_empty() && synced != base {
        // API host changed — drop previous server branding.
        clear_server_branding();
    }

    {
        let mut last = LAST_BRANDING_POLL.lock().unwrap();
        if let Some(t) = *last {
            if t.elapsed() < BRANDING_POLL_INTERVAL {
                return;
            }
        }
        *last = Some(Instant::now());
    }

    let value = match fetch_branding().await {
        Ok(v) => v,
        Err(err) => {
            log::debug!("branding fetch skipped: {err}");
            return;
        }
    };
    let payload: BrandingPayload = match serde_json::from_value(value) {
        Ok(p) => p,
        Err(err) => {
            log::warn!("branding payload parse failed: {err}");
            return;
        }
    };
    let local_rev = LocalConfig::get_option(OPTION_BRANDING_REVISION);
    if !payload.revision.is_empty()
        && payload.revision == local_rev
        && synced == base
        && is_server_managed()
    {
        return;
    }
    if let Err(err) = apply_branding_payload(&base, payload).await {
        log::warn!("branding apply failed: {err}");
    }
}

/// Whether Settings → Branding should be read-only (managed by server).
#[allow(dead_code)]
pub fn is_branding_managed_by_server() -> bool {
    is_server_managed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusal_is_none_for_success_answers() {
        assert_eq!(classify_refusal(200, None, "", false), None);
        assert_eq!(classify_refusal(202, None, "Waiting for operator approval", false), None);
    }

    #[test]
    fn node_panel_codes_are_mapped() {
        assert_eq!(
            classify_refusal(403, Some("device_banned"), "This device is banned", false),
            Some("banned")
        );
        assert_eq!(
            classify_refusal(403, Some("device_removed"), "This device has been removed", false),
            Some("removed")
        );
    }

    #[test]
    fn missing_admission_maps_to_awaiting_or_rejected_token() {
        // No token pasted: the operator still has to approve or supply one.
        assert_eq!(
            classify_refusal(403, Some("enrollment_proof_required"), "", false),
            Some("awaiting-token-or-approval")
        );
        // A token was pasted and the server still refused it.
        assert_eq!(
            classify_refusal(403, Some("enrollment_proof_required"), "", true),
            Some("token-rejected")
        );
    }

    #[test]
    fn go_server_messages_are_mapped_without_error_codes() {
        // The Go server answers `status: "rejected"` and carries no `error`.
        assert_eq!(
            classify_refusal(403, None, "Device is banned", false),
            Some("banned")
        );
        assert_eq!(
            classify_refusal(403, None, "Device has been removed", false),
            Some("removed")
        );
        assert_eq!(
            classify_refusal(403, None, "Device enrollment was rejected", false),
            Some("rejected")
        );
        assert_eq!(
            classify_refusal(
                403,
                None,
                "Enrollment is locked — a valid token is required",
                false
            ),
            Some("awaiting-token-or-approval")
        );
        assert_eq!(
            classify_refusal(
                403,
                None,
                "Enrollment is locked — a valid token is required",
                true
            ),
            Some("token-rejected")
        );
    }

    #[test]
    fn operator_action_states_back_off() {
        // These cannot change without an operator, so retrying every two minutes
        // only floods the server with refusals.
        for state in ["banned", "removed", "identity-conflict", "token-rejected"] {
            assert!(is_operator_action_state(state), "{state} should back off");
        }
        // These can change on their own (operator approves, token arrives).
        for state in ["pending", "unregistered", "awaiting-token-or-approval", "approved"] {
            assert!(!is_operator_action_state(state), "{state} should retry soon");
        }
    }

    #[test]
    fn enrolment_state_covers_every_mapped_state() {
        // Guards against a mapped state that the diagnostics card cannot name.
        for state in [
            "approved",
            "pending",
            "rejected",
            "banned",
            "removed",
            "identity-conflict",
            "awaiting-token-or-approval",
            "token-rejected",
        ] {
            LocalConfig::set_option(OPTION_ENROLLMENT_STATUS.to_owned(), state.to_owned());
            assert_eq!(enrollment_state(), state);
        }
        LocalConfig::set_option(OPTION_ENROLLMENT_STATUS.to_owned(), String::new());
        LocalConfig::set_option(OPTION_ENROLLMENT_TOKEN.to_owned(), String::new());
        assert_eq!(enrollment_state(), "unregistered");
    }
}
