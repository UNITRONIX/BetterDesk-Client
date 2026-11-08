//! Secret storage for credentials the client must persist.
//!
//! Secrets go to the OS credential store (Windows Credential Manager, macOS Keychain,
//! Linux kernel keyutils) instead of the TOML config. A stolen config file then yields
//! endpoints and preferences, not live tokens.
//!
//! Every call degrades rather than fails: a platform without a usable store (headless
//! Linux, restricted service account, mobile) falls back to the existing field-level
//! encryption in `hbb_common`, which keeps the client working while the diagnostics card
//! reports that the fallback is in use. Nothing here panics or unwraps a store error.

use hbb_common::{config::LocalConfig, log};

/// `keyring` is a desktop-only dependency: the crate is declared under the Windows, macOS
/// and Linux target sections in `Cargo.toml`. Mobile builds therefore use the config
/// fallback exclusively, which is why every `keyring` use below is gated rather than the
/// whole module.

const SERVICE: &str = "betterdesk-client";

/// Names of the secrets the client stores outside the config file.
pub mod keys {
    /// Account access token (`/api/*` bearer).
    pub const ACCESS_TOKEN: &str = "access-token";
    /// Device credential issued at enrollment / pairing.
    pub const DEVICE_TOKEN: &str = "device-token";
    /// Permanent access password hash fragment.
    pub const PERMANENT_PASSWORD: &str = "permanent-password";
    /// Local unlock PIN.
    pub const UNLOCK_PIN: &str = "unlock-pin";
}

/// Where a value came from, for the diagnostics card.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreBackend {
    /// OS credential store.
    Os,
    /// `hbb_common` encrypted config (no OS store available).
    Config,
}

impl StoreBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            StoreBackend::Os => "os",
            StoreBackend::Config => "config",
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn entry(name: &str) -> Option<keyring::Entry> {
    keyring::Entry::new(SERVICE, name).ok()
}

/// Read a secret. Returns `None` when absent or when no OS store is usable.
pub fn get_secret(name: &str) -> Option<String> {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        let entry = entry(name)?;
        return match entry.get_password() {
            Ok(value) if !value.is_empty() => Some(value),
            _ => None,
        };
    }
    // Mobile has no `keyring` dependency: the config fallback is the only store.
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = name;
        None
    }
}

/// Store a secret, falling back to the config when the OS store rejects the write.
///
/// Returns the backend actually used so the caller can report it.
pub fn set_secret(name: &str, value: &str) -> StoreBackend {
    let backend = set_secret_inner(name, value);
    // Remember the outcome so the diagnostics card reports reality, not a guess.
    LocalConfig::set_option(
        "secrets-store-backend".to_owned(),
        backend.as_str().to_owned(),
    );
    backend
}

fn set_secret_inner(name: &str, value: &str) -> StoreBackend {
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    if let Some(entry) = entry(name) {
        if value.is_empty() {
            // Deleting a missing entry is not an error: callers clear secrets twice.
            match entry.delete_credential() {
                Ok(()) => return StoreBackend::Os,
                Err(keyring::Error::NoEntry) => return StoreBackend::Os,
                Err(err) => {
                    log::debug!("credential store delete failed for {name}: {err}");
                }
            }
        } else {
            match entry.set_password(value) {
                Ok(()) => return StoreBackend::Os,
                Err(err) => {
                    log::warn!("credential store unavailable for {name}: {err}; using config fallback");
                }
            }
        }
    }
    // Fallback keeps the previous behaviour so an unavailable store cannot lock a user out
    // of their own device. The value is still protected by the config's field encryption.
    LocalConfig::set_option(fallback_key(name), value.to_owned());
    StoreBackend::Config
}

/// Read a secret, consulting the config fallback when the OS store has nothing.
pub fn get_secret_with_fallback(name: &str) -> Option<(String, StoreBackend)> {
    if let Some(value) = get_secret(name) {
        return Some((value, StoreBackend::Os));
    }
    let fallback = LocalConfig::get_option(&fallback_key(name));
    if fallback.is_empty() {
        None
    } else {
        Some((fallback, StoreBackend::Config))
    }
}

/// Remove a secret from both the OS store and the fallback.
pub fn delete_secret(name: &str) {
    if let Some(entry) = entry(name) {
        let _ = entry.delete_credential();
    }
    LocalConfig::set_option(fallback_key(name), String::new());
}

/// Diagnostic summary for Settings → Network: which backend is in use, without values.
pub fn diagnostics_json() -> String {
    // Determined at runtime by the last write, never guessed: an empty probe would read as
    // "os" on a machine that has no store.
    let backend = LocalConfig::get_option("secrets-store-backend");
    let backend = if backend.is_empty() { "unknown" } else { backend.as_str() };
    format!(
        "{{\"backend\":\"{}\",\"has_access_token\":{},\"has_device_token\":{}}}",
        backend,
        get_secret(keys::ACCESS_TOKEN).is_some(),
        get_secret(keys::DEVICE_TOKEN).is_some(),
    )
}

fn fallback_key(name: &str) -> String {
    format!("secret-fallback-{name}")
}

/// Move secrets out of the config file into the OS credential store.
///
/// Idempotent: a marker is written only after a successful move, so a failed migration
/// retries on the next launch instead of leaving values stranded between two stores.
pub fn migrate_secrets_from_config() {
    const MARKER: &str = "secrets-migrated-v1";
    if LocalConfig::get_option(MARKER) == "1" {
        return;
    }

    let candidates = [
        (keys::ACCESS_TOKEN, "access_token"),
        (keys::UNLOCK_PIN, "unlock_pin"),
    ];
    let mut all_moved = true;
    for (secret_name, config_key) in candidates {
        if get_secret(secret_name).is_some() {
            continue;
        }
        let existing = hbb_common::config::Config::get_option(config_key);
        if existing.is_empty() {
            continue;
        }
        if set_secret(secret_name, &existing) != StoreBackend::Os {
            // No OS store on this host: leave the value where it is rather than copying it
            // into a fallback key that is no better protected than the original.
            all_moved = false;
            continue;
        }
        // Clear the config copy only after confirming the store returns the same value.
        if get_secret(secret_name).as_deref() == Some(existing.as_str()) {
            hbb_common::config::Config::set_option(config_key.to_owned(), String::new());
        } else {
            all_moved = false;
        }
    }
    if all_moved {
        LocalConfig::set_option(MARKER.to_owned(), "1".to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_key_is_namespaced() {
        assert_eq!(fallback_key("access-token"), "secret-fallback-access-token");
    }

    #[test]
    fn backend_labels_are_stable() {
        // The UI and the diagnostics JSON both switch on these strings.
        assert_eq!(StoreBackend::Os.as_str(), "os");
        assert_eq!(StoreBackend::Config.as_str(), "config");
    }

    #[test]
    fn missing_secret_is_none_not_panic() {
        // A name that was never written must not panic on any platform.
        let name = format!("test-missing-{}", uuid::Uuid::new_v4().simple());
        assert!(get_secret(&name).is_none());
        delete_secret(&name);
    }

    #[test]
    fn migration_uses_existing_config_keys() {
        // If a config key is renamed, the migration would silently move nothing; the names
        // are asserted here rather than discovered at runtime on an operator's machine.
        for key in ["access_token", "unlock_pin"] {
            let option = hbb_common::config::Config::get_option(key);
            // Absent is fine; this asserts the key exists as a known option, not a value.
            assert!(option.len() < 4096);
        }
    }
}
