//! Resolution of the BetterDesk client API base URL.
//!
//! The API host normally comes from the ID (rendezvous) server with port -2, but that
//! derivation assumed plain HTTP. A server that only serves HTTPS on 443 (reverse proxy)
//! was therefore unreachable: branding, enrollment and pairing all failed while remote
//! sessions still worked, which is the most confusing failure this client has.
//!
//! This module probes candidates HTTPS-first, caches the winner in LocalConfig, and
//! refuses to silently fall back to plaintext unless the operator opted in.

use hbb_common::{
    config::{keys, Config, LocalConfig},
    log,
    tokio::time::timeout,
    ResultType,
};
use std::time::{Duration, Instant};

use crate::hbbs_http::create_http_client_async_with_url;

/// How long a resolved base stays valid before it is probed again.
pub const RESOLVE_TTL: Duration = Duration::from_secs(60 * 60);
/// Per-candidate request timeout. Keeps a dead host from stalling the caller.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(4);
/// Upper bound for a whole resolution, so a black-holed network cannot hang startup.
pub const RESOLVE_BUDGET: Duration = Duration::from_secs(12);

const OPTION_RESOLVED_BASE: &str = "api-base-resolved";
const OPTION_RESOLVED_AT: &str = "api-base-resolved-at";
const OPTION_RESOLVED_SOURCE: &str = "api-base-resolved-source";

/// Where a resolved base came from, for the diagnostics card.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaseSource {
    /// Explicit `api-server` option: used verbatim, never rewritten.
    Explicit,
    /// Resolved from the ID server by probing candidates.
    Derived,
    /// Nothing configured.
    None,
}

impl BaseSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            BaseSource::Explicit => "explicit",
            BaseSource::Derived => "derived",
            BaseSource::None => "none",
        }
    }

    fn parse(value: &str) -> BaseSource {
        match value {
            "explicit" => BaseSource::Explicit,
            "derived" => BaseSource::Derived,
            _ => BaseSource::None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedApiBase {
    pub base: String,
    pub source: BaseSource,
    pub scheme: String,
    /// True when the base is plaintext HTTP, i.e. the operator opted in.
    pub insecure: bool,
    /// Measured round trip for the accepted candidate; 0 when served from cache.
    pub latency_ms: u64,
    pub from_cache: bool,
    pub last_error: Option<String>,
}

impl ResolvedApiBase {
    fn empty() -> Self {
        Self {
            base: String::new(),
            source: BaseSource::None,
            scheme: String::new(),
            insecure: false,
            latency_ms: 0,
            from_cache: false,
            last_error: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.base.is_empty()
    }

    /// JSON for the FFI diagnostics card. Hand-built to avoid pulling serde into a hot path.
    pub fn to_json(&self) -> String {
        let escape = |value: &str| {
            value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        };
        format!(
            "{{\"base\":\"{}\",\"source\":\"{}\",\"scheme\":\"{}\",\"insecure\":{},\
             \"latency_ms\":{},\"from_cache\":{},\"last_error\":{}}}",
            escape(&self.base),
            self.source.as_str(),
            escape(&self.scheme),
            self.insecure,
            self.latency_ms,
            self.from_cache,
            match self.last_error.as_deref() {
                Some(err) => format!("\"{}\"", escape(err)),
                None => "null".to_owned(),
            }
        )
    }
}

/// Host part of the configured ID server, without scheme, path, or port.
///
/// Accepts `host`, `host:port`, `https://host/`, and a comma-separated list (first wins),
/// mirroring what operators actually paste into Settings → Network.
pub fn rendezvous_hosts(raw: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    for entry in raw.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let without_scheme = match entry.split_once("://") {
            Some((_, rest)) => rest,
            None => entry,
        };
        let authority = without_scheme
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .trim();
        if authority.is_empty() || authority.contains('@') {
            continue;
        }
        // Strip the port, keeping IPv6 literals (`[::1]:21116`) intact.
        let host = if let Some(rest) = authority.strip_prefix('[') {
            match rest.split_once(']') {
                Some((host, _)) => format!("[{host}]"),
                None => continue,
            }
        } else {
            authority.split(':').next().unwrap_or_default().to_owned()
        };
        if host.is_empty() {
            continue;
        }
        let host = host.to_ascii_lowercase();
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    hosts
}

fn origin(host: &str) -> String {
    // A bare IPv6 literal needs brackets inside a URL.
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

fn allow_insecure(base: &str) -> bool {
    crate::common::get_builtin_option(keys::OPTION_ALLOW_INSECURE_TLS_FALLBACK) == "Y"
        || base.starts_with("http://") && !Config::get_option("api-server").is_empty()
}

/// Ordered candidates for derivated resolution. HTTPS first, plaintext only last.
fn candidates(host: &str) -> Vec<String> {
    let host = origin(host);
    let allow_https_21114 = crate::common::get_builtin_option(
        hbb_common::config::keys::OPTION_ALLOW_HTTPS_21114,
    ) == "Y";
    let mut out = vec![format!("https://{host}")];
    if allow_https_21114 {
        out.push(format!("https://{host}:21114"));
    }
    out.push(format!("https://{host}:5443"));
    out.push(format!("http://{host}:21114"));
    out.push(format!("http://{host}:21121"));
    out
}

/// Probe one base URL. Returns the measured latency on success.
async fn probe(base: &str) -> ResultType<u128> {
    let url = format!("{base}/api/health");
    let started = Instant::now();
    let client = create_http_client_async_with_url(&url).await;
    let response = timeout(PROBE_TIMEOUT, client.get(&url).send())
        .await
        .map_err(|_| hbb_common::anyhow::anyhow!("probe timed out after {PROBE_TIMEOUT:?}"))??;
    if !response.status().is_success() {
        hbb_common::bail!("HTTP {}", response.status());
    }
    Ok(started.elapsed().as_millis())
}

/// Cached, still-valid base for synchronous callers (Flutter `mainGetApiServer`, account,
/// AB). Returns `None` when nothing has been resolved yet, so callers keep the old
/// derivation instead of losing their endpoint.
pub fn cached_base() -> Option<String> {
    cached().map(|resolved| resolved.base).filter(|b| !b.is_empty())
}

fn cached() -> Option<ResolvedApiBase> {
    let base = LocalConfig::get_option(OPTION_RESOLVED_BASE);
    if base.is_empty() {
        return None;
    }
    let stored_at = LocalConfig::get_option(OPTION_RESOLVED_AT);
    let age = stored_at
        .parse::<u64>()
        .ok()
        .and_then(|secs| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok().map(|now| now.as_secs().saturating_sub(secs)));
    if age.map(|age| age > RESOLVE_TTL.as_secs()).unwrap_or(true) {
        return None;
    }
    // A cache entry for a different ID server must not leak across configurations.
    let host = rendezvous_hosts(&Config::get_option(
        "custom-rendezvous-server",
    ))
    .into_iter()
    .next()
    .unwrap_or_default();
    if !host.is_empty() && !base.to_ascii_lowercase().contains(&host) {
        return None;
    }
    Some(ResolvedApiBase {
        scheme: scheme_of(&base),
        insecure: base.starts_with("http://"),
        source: BaseSource::parse(&LocalConfig::get_option(OPTION_RESOLVED_SOURCE)),
        from_cache: true,
        ..ResolvedApiBase {
            base,
            ..ResolvedApiBase::empty()
        }
    })
}

fn scheme_of(base: &str) -> String {
    base.split("://").next().unwrap_or_default().to_owned()
}

fn store(result: &ResolvedApiBase) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    LocalConfig::set_option(OPTION_RESOLVED_BASE.to_owned(), result.base.clone());
    LocalConfig::set_option(OPTION_RESOLVED_AT.to_owned(), now.to_string());
    LocalConfig::set_option(
        OPTION_RESOLVED_SOURCE.to_owned(),
        result.source.as_str().to_owned(),
    );
}

/// Drop the cached base, e.g. when the operator edits the server or after an update.
pub fn reset_cache() {
    for key in [OPTION_RESOLVED_BASE, OPTION_RESOLVED_AT, OPTION_RESOLVED_SOURCE] {
        LocalConfig::set_option(key.to_owned(), String::new());
    }
}

/// Cached state as diagnostics JSON, without any network traffic.
pub fn diagnostics_json() -> String {
    match cached() {
        Some(resolved) => resolved.to_json(),
        None => ResolvedApiBase::empty().to_json(),
    }
}

/// Resolve the API base for the configured server.
///
/// An explicit `api-server` option is authoritative and used as-is: the operator may point
/// at a host that does not answer `/api/health`, and rewriting it would break working setups.
/// Otherwise the ID server is probed HTTPS-first, and the first candidate that answers
/// `/api/health` wins.
pub async fn resolve(force: bool) -> ResolvedApiBase {
    if Config::no_register_device() {
        return ResolvedApiBase::empty();
    }

    let explicit = Config::get_option("api-server")
        .trim()
        .trim_end_matches('/')
        .to_owned();
    if !explicit.is_empty() {
        return ResolvedApiBase {
            scheme: scheme_of(&explicit),
            insecure: explicit.starts_with("http://"),
            base: explicit,
            source: BaseSource::Explicit,
            ..ResolvedApiBase::empty()
        };
    }

    if !force {
        if let Some(cached) = cached() {
            return cached;
        }
    }

    let hosts = rendezvous_hosts(&Config::get_option(
        "custom-rendezvous-server",
    ));
    let Some(host) = hosts.first() else {
        return ResolvedApiBase::empty();
    };

    let deadline = Instant::now() + RESOLVE_BUDGET;
    let mut last_error = None;
    for candidate in candidates(host) {
        if Instant::now() >= deadline {
            last_error = Some("resolution budget exhausted".to_owned());
            break;
        }
        if candidate.starts_with("http://") && !allow_insecure(&candidate) {
            last_error = Some(format!(
                "{candidate} is plaintext and TLS fallback is disabled \
                 (enable 'Allow insecure TLS fallback' to permit it)"
            ));
            continue;
        }
        match probe(&candidate).await {
            Ok(latency_ms) => {
                let resolved = ResolvedApiBase {
                    scheme: scheme_of(&candidate),
                    insecure: candidate.starts_with("http://"),
                    base: candidate,
                    source: BaseSource::Derived,
                    latency_ms: latency_ms as u64,
                    from_cache: false,
                    last_error: None,
                };
                if resolved.insecure {
                    log::warn!(
                        "BetterDesk API resolved over plaintext HTTP ({}); configure HTTPS",
                        resolved.base
                    );
                } else {
                    log::info!("BetterDesk API resolved: {}", resolved.base);
                }
                store(&resolved);
                return resolved;
            }
            Err(err) => {
                last_error = Some(format!("{candidate}: {err}"));
            }
        }
    }

    // Keep the previous answer usable when a transient probe failure occurs; the caller
    // still sees the error so the diagnostics card can say what is wrong.
    let mut stale = cached().unwrap_or_else(ResolvedApiBase::empty);
    stale.from_cache = stale.is_empty();
    stale.last_error = last_error;
    stale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendezvous_hosts_accepts_what_operators_paste() {
        assert_eq!(rendezvous_hosts("desk.example.com"), vec!["desk.example.com"]);
        assert_eq!(
            rendezvous_hosts("https://desk.example.com/"),
            vec!["desk.example.com"]
        );
        assert_eq!(
            rendezvous_hosts("desk.example.com:21116"),
            vec!["desk.example.com"]
        );
        assert_eq!(
            rendezvous_hosts("a.example.com, b.example.com"),
            vec!["a.example.com", "b.example.com"]
        );
        // IPv6 literal keeps its brackets.
        assert_eq!(rendezvous_hosts("[2001:db8::1]:21116"), vec!["[2001:db8::1]"]);
        // Credentials and empty entries are dropped, never used to build a base URL.
        assert!(rendezvous_hosts("user@host").is_empty());
        assert!(rendezvous_hosts("   ").is_empty());
    }

    #[test]
    fn candidates_try_https_before_plaintext() {
        let list = candidates("desk.example.com");
        assert_eq!(list[0], "https://desk.example.com");
        let first_http = list
            .iter()
            .position(|c| c.starts_with("http://"))
            .expect("plaintext candidates exist as a last resort");
        assert!(list[..first_http].iter().all(|c| c.starts_with("https://")));
        assert!(list.iter().any(|c| c == "http://desk.example.com:21114"));
        assert!(list.iter().any(|c| c == "http://desk.example.com:21121"));
    }

    #[test]
    fn candidate_ipv6_literals_are_bracketed_once() {
        let list = candidates("[2001:db8::1]");
        assert_eq!(list[0], "https://[2001:db8::1]");
    }

    #[test]
    fn json_reports_insecure_and_error() {
        let resolved = ResolvedApiBase {
            base: "http://desk.example.com:21114".to_owned(),
            scheme: "http".to_owned(),
            insecure: true,
            source: BaseSource::Derived,
            latency_ms: 12,
            from_cache: false,
            last_error: Some("boom \"quoted\"".to_owned()),
        };
        let json = resolved.to_json();
        assert!(json.contains("\"insecure\":true"));
        assert!(json.contains("\"scheme\":\"http\""));
        assert!(json.contains("boom \\\"quoted\\\""));
    }
}
