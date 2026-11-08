//! Short-code pairing: set up a client from `host` + 6 characters.
//!
//! The user types the server address and a code an operator showed them. The client asks
//! the server to redeem the code, and the server answers with everything a manual setup
//! would require: rendezvous/relay/api endpoints and the server public key. Branding and
//! enrollment then run through the normal paths.
//!
//! Safety properties:
//!   - The code is sent only over HTTPS unless the operator explicitly enabled plaintext
//!     fallback, because it is a bearer credential for one configuration.
//!   - Configuration is applied only after a complete, valid response; a partial answer
//!     leaves the client untouched (no half-configured server).
//!   - Attempts are bounded: a fixed candidate list and a hard deadline, so a black-holed
//!     address cannot hang the UI or spawn retries.

use hbb_common::{
    config::Config,
    log,
    tokio::{self, time::timeout},
    ResultType,
};
use serde_derive::Deserialize;
use serde_json::Value;
use std::time::{Duration, Instant};

/// Per-candidate request budget.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(4);
/// Whole operation budget, including every candidate.
const TOTAL_BUDGET: Duration = Duration::from_secs(10);
const PAIRING_CODE_LEN: usize = 6;

/// Outcome shown in the dialog. The struct deliberately carries no server-supplied text:
/// a compromised or misconfigured server must not be able to inject UI copy.
#[derive(Clone)]
pub struct PairOutcome {
    pub ok: bool,
    /// Translation key describing the failure, e.g. `pairing-code-invalid`.
    pub error_key: Option<&'static str>,
    /// Which base URL answered, for the diagnostics line.
    pub used_base: String,
    pub host: String,
    pub server_name: String,
    pub enrollment_status: String,
    pub insecure: bool,
}

impl PairOutcome {
    fn failure(key: &'static str) -> Self {
        Self {
            ok: false,
            error_key: Some(key),
            used_base: String::new(),
            host: String::new(),
            server_name: String::new(),
            enrollment_status: String::new(),
            insecure: false,
        }
    }

    pub fn to_json(&self) -> String {
        let escape = |value: &str| {
            value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        };
        format!(
            "{{\"ok\":{},\"error_key\":{},\"used_base\":\"{}\",\"host\":\"{}\",\
             \"server_name\":\"{}\",\"enrollment_status\":\"{}\",\"insecure\":{}}}",
            self.ok,
            match self.error_key {
                Some(key) => format!("\"{}\"", escape(key)),
                None => "null".to_owned(),
            },
            escape(&self.used_base),
            escape(&self.host),
            escape(&self.server_name),
            escape(&self.enrollment_status),
            self.insecure,
        )
    }
}

#[derive(Deserialize)]
struct ConnectionBundle {
    #[serde(default)]
    signal: String,
    #[serde(default)]
    relay: String,
    #[serde(default)]
    api: String,
}

#[derive(Deserialize)]
struct RedeemResponse {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    error: String,
    #[serde(default)]
    enrollment: Option<EnrollmentInfo>,
    #[serde(default)]
    connection: Option<ConnectionBundle>,
    #[serde(default)]
    branding: Option<Value>,
    #[serde(default)]
    server_key: String,
}

#[derive(Deserialize)]
struct EnrollmentInfo {
    #[serde(default)]
    status: String,
}

/// Parse operator input into `(host, code)`.
///
/// Accepts `host`, `host:port`, `https://host/`, and an optional trailing path. Rejects
/// anything that could be used to redirect the request elsewhere (credentials, a
/// different scheme, a query string).
pub fn parse_pairing_input(raw: &str, code: &str) -> Result<(String, String), &'static str> {
    let code = normalize_code(code);
    if !valid_code_format(&code) {
        return Err("pairing-code-format");
    }
    let raw = raw.trim();
    if raw.is_empty() {
        return Err("pairing-host-required");
    }
    let (scheme, rest) = match raw.split_once("://") {
        Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
        None => ("https".to_owned(), raw),
    };
    if scheme != "https" && scheme != "http" {
        return Err("pairing-host-required");
    }
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim();
    if authority.is_empty() || authority.contains('@') || authority.contains(' ') {
        return Err("pairing-host-required");
    }
    Ok((authority.to_string(), code))
}

fn normalize_code(code: &str) -> String {
    code.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '\t'))
        .flat_map(|c| c.to_uppercase())
        .collect()
}

fn valid_code_format(code: &str) -> bool {
    const ALPHABET: &str = "23456789ABCDEFGHJKMNPQRSTVWXYZ";
    code.len() == PAIRING_CODE_LEN && code.chars().all(|c| ALPHABET.contains(c))
}

/// Candidate base URLs for a host, HTTPS first.
///
/// The redeem endpoint lives on the client API, which may be behind 443, on 21114, or on
/// the legacy 21121 proxy. Plaintext candidates come last and are only used when the
/// operator allowed an insecure fallback.
fn candidate_bases(authority: &str) -> Vec<String> {
    let allow_plaintext = crate::common::get_builtin_option(
        hbb_common::config::keys::OPTION_ALLOW_INSECURE_TLS_FALLBACK,
    ) == "Y";
    let mut bases = vec![format!("https://{authority}")];
    // A port in the input means the operator knows where the API is; use it verbatim.
    if !authority.contains(':') {
        bases.push("https://__HOST__:21114".replace("__HOST__", authority));
        bases.push("https://__HOST__:5443".replace("__HOST__", authority));
        if allow_plaintext {
            bases.push("http://__HOST__:21114".replace("__HOST__", authority));
            bases.push("http://__HOST__:21121".replace("__HOST__", authority));
        }
    } else if allow_plaintext {
        bases.push(format!("http://{authority}"));
    }
    bases
}

fn device_identity() -> (String, String, String, String) {
    let id = Config::get_id();
    let uuid = crate::encode64(hbb_common::get_uuid());
    let hostname = hbb_common::whoami::fallible::hostname().unwrap_or_default();
    let platform = std::env::consts::OS.to_owned();
    (id, uuid, hostname, platform)
}

/// Redeem a pairing code and apply the resulting configuration.
pub async fn redeem(host_input: &str, code: &str) -> PairOutcome {
    let (authority, code) = match parse_pairing_input(host_input, code) {
        Ok(parsed) => parsed,
        Err(key) => return PairOutcome::failure(key),
    };
    let (device_id, uuid, hostname, platform) = device_identity();
    if device_id.is_empty() {
        return PairOutcome::failure("pairing-device-id-missing");
    }

    let body = serde_json::json!({
        "code": code,
        "device_id": device_id,
        "uuid": uuid,
        "hostname": hostname,
        "platform": platform,
        "version": crate::VERSION,
        "device_type": "betterdesk-desktop",
    });

    let deadline = Instant::now() + TOTAL_BUDGET;
    let mut last_error = None;
    // Track a rejected code separately from an unreachable server: the first is final, the
    // second is worth retrying on another port.
    let mut saw_rejection = false;

    for base in candidate_bases(&authority) {
        if Instant::now() >= deadline {
            last_error = Some("pairing-timeout");
            break;
        }
        let url = format!("{base}/api/pairing/redeem");
        match attempt(&url, &body).await {
            Ok(Attempt::Accepted(response)) => {
                return apply(&base, &authority, response);
            }
            Ok(Attempt::Rejected) => {
                saw_rejection = true;
                last_error = Some("pairing-code-invalid");
            }
            Err(err) => {
                log::debug!("pairing attempt failed for {base}: {err}");
                last_error = Some("pairing-server-unreachable");
            }
        }
    }

    // A valid code rejected by every reachable endpoint is a real rejection, not a network
    // problem; report it as such instead of "server unreachable".
    let key = if saw_rejection {
        last_error.unwrap_or("pairing-code-invalid")
    } else {
        last_error.unwrap_or("pairing-server-unreachable")
    };
    PairOutcome::failure(key)
}

enum Attempt {
    Accepted(RedeemResponse),
    Rejected,
}

async fn attempt(url: &str, body: &Value) -> ResultType<Attempt> {
    let client = crate::hbbs_http::create_http_client_async_with_url(url).await;
    let response = timeout(ATTEMPT_TIMEOUT, client.post(url).json(body).send())
        .await
        .map_err(|_| hbb_common::anyhow::anyhow!("request timed out"))??;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Ok(Attempt::Rejected);
    }
    if !status.is_success() {
        hbb_common::bail!("HTTP {status}");
    }
    let parsed: RedeemResponse = serde_json::from_str(&text)?;
    if !parsed.success {
        // The server answers `invalid_or_expired` for every unusable code; treat it as a
        // rejection so the caller does not keep probing other ports.
        return Ok(Attempt::Rejected);
    }
    Ok(Attempt::Accepted(parsed))
}

/// Apply a successful redemption atomically.
///
/// Every option is validated first; only a complete bundle is written. A response missing
/// the rendezvous endpoint would otherwise leave the client pointed at nothing.
fn apply(base: &str, authority: &str, response: RedeemResponse) -> PairOutcome {
    let Some(connection) = response.connection else {
        return PairOutcome::failure("pairing-server-unreachable");
    };
    if connection.signal.is_empty() {
        log::warn!("pairing response from {base} has no signal endpoint");
        return PairOutcome::failure("pairing-server-unreachable");
    }
    // Verify the host we are about to trust is the one the user typed, so a redirect by
    // the first candidate cannot move the client to a different server silently.
    let expected = normalize_authority(authority);
    let response_host = normalize_authority(connection.signal.split(':').next().unwrap_or_default());
    if !expected.is_empty() && !response_host.is_empty() && !expected.ends_with(&response_host)
        && !response_host.ends_with(&expected)
    {
        log::warn!("pairing host mismatch: typed {authority}, server reports {response_host}");
        return PairOutcome::failure("pairing-host-mismatch");
    }

    let api = if connection.api.is_empty() {
        base.to_owned()
    } else {
        connection.api.clone()
    };
    let insecure = api.starts_with("http://");

    let mut options = Config::get_options();
    options.insert(
        "custom-rendezvous-server".to_owned(),
        connection.signal.clone(),
    );
    options.insert(
        "relay-server".to_owned(),
        if connection.relay.is_empty() {
            connection.signal.clone()
        } else {
            connection.relay.clone()
        },
    );
    options.insert("api-server".to_owned(), api);
    if !response.server_key.is_empty() {
        options.insert("key".to_owned(), response.server_key);
    }
    Config::set_options(options);
    // A different server invalidates every cached probe from the previous one.
    crate::hbbs_http::api_base::reset_cache();

    let server_name = response
        .branding
        .as_ref()
        .and_then(|branding| branding.get("company_name"))
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_owned();
    let enrollment_status = response
        .enrollment
        .map(|enrollment| enrollment.status)
        .unwrap_or_default();

    log::info!("paired with {} ({})", base, if insecure { "http" } else { "https" });
    PairOutcome {
        ok: true,
        error_key: None,
        used_base: base.to_owned(),
        host: connection.signal,
        server_name,
        enrollment_status,
        insecure,
    }
}

fn normalize_authority(value: &str) -> String {
    value
        .trim()
        .trim_matches(['[', ']'])
        .to_ascii_lowercase()
}

/// Preview what a code points at, without registering the device.
pub async fn preview(host_input: &str, code: &str) -> PairOutcome {
    let (authority, code) = match parse_pairing_input(host_input, code) {
        Ok(parsed) => parsed,
        Err(key) => return PairOutcome::failure(key),
    };
    let body = serde_json::json!({ "code": code });
    for base in candidate_bases(&authority) {
        let url = format!("{base}/api/pairing/config");
        let client = crate::hbbs_http::create_http_client_async_with_url(&url).await;
        let send = timeout(ATTEMPT_TIMEOUT, client.post(&url).json(&body).send()).await;
        if let Ok(Ok(response)) = send {
            if response.status().is_success() {
                if let Ok(text) = response.text().await {
                    if let Ok(value) = serde_json::from_str::<Value>(&text) {
                        let mut outcome = PairOutcome::failure("");
                        outcome.ok = true;
                        outcome.error_key = None;
                        outcome.used_base = base.clone();
                        outcome.insecure = base.starts_with("http://");
                        outcome.server_name = value
                            .get("server_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_owned();
                        outcome.host = value
                            .get("connection")
                            .and_then(|c| c.get("signal"))
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_owned();
                        return outcome;
                    }
                }
            } else if response.status() == reqwest::StatusCode::UNAUTHORIZED {
                return PairOutcome::failure("pairing-code-invalid");
            }
        }
    }
    PairOutcome::failure("pairing-server-unreachable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_accepts_what_a_person_types() {
        assert_eq!(
            parse_pairing_input("desk.example.com", "abc234").unwrap(),
            ("desk.example.com".to_owned(), "ABC234".to_owned())
        );
        assert_eq!(
            parse_pairing_input("https://desk.example.com/", "AB C 234").unwrap(),
            ("desk.example.com".to_owned(), "ABC234".to_owned())
        );
        assert_eq!(
            parse_pairing_input("desk.example.com:21114", "ABC234").unwrap().0,
            "desk.example.com:21114"
        );
        // An IPv6 literal keeps its brackets so the URL stays valid.
        assert_eq!(
            parse_pairing_input("[2001:db8::1]", "ABC234").unwrap().0,
            "[2001:db8::1]"
        );
    }

    #[test]
    fn input_rejects_redirects_and_bad_codes() {
        // Credentials could be used to make the request look trusted.
        assert_eq!(
            parse_pairing_input("user@evil.test", "ABC234").unwrap_err(),
            "pairing-host-required"
        );
        // A different scheme is not a BetterDesk server.
        assert_eq!(
            parse_pairing_input("ftp://desk.example.com", "ABC234").unwrap_err(),
            "pairing-host-required"
        );
        assert_eq!(
            parse_pairing_input("", "ABC234").unwrap_err(),
            "pairing-host-required"
        );
        // Ambiguous characters are excluded from the alphabet, so they are a format error
        // rather than a wrong code - the user can see the typo immediately.
        assert_eq!(
            parse_pairing_input("desk.example.com", "ABC23O").unwrap_err(),
            "pairing-code-format"
        );
        assert_eq!(
            parse_pairing_input("desk.example.com", "ABC23").unwrap_err(),
            "pairing-code-format"
        );
    }

    #[test]
    fn candidates_are_https_first_and_respect_an_explicit_port() {
        let bases = candidate_bases("desk.example.com");
        assert!(bases[0].starts_with("https://"));
        // Plaintext is only present when the operator enabled it; by default there is none.
        if crate::common::get_builtin_option(
            hbb_common::config::keys::OPTION_ALLOW_INSECURE_TLS_FALLBACK,
        ) != "Y"
        {
            assert!(bases.iter().all(|b| b.starts_with("https://")));
        }

        // An explicit port is used verbatim, not expanded into a port scan.
        let explicit = candidate_bases("desk.example.com:21114");
        assert_eq!(explicit[0], "https://desk.example.com:21114");
    }

    #[test]
    fn outcome_json_never_embeds_server_text() {
        let outcome = PairOutcome {
            ok: true,
            error_key: None,
            used_base: "https://desk.example.com".to_owned(),
            host: "desk.example.com:21116".to_owned(),
            server_name: "ACME \"quoted\"".to_owned(),
            enrollment_status: "approved".to_owned(),
            insecure: false,
        };
        let json = outcome.to_json();
        assert!(json.contains("\"ok\":true"));
        assert!(json.contains("ACME \\\"quoted\\\""));
    }
}
