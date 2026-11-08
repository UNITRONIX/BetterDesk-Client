# Changelog — BetterDesk Client

## [Unreleased]

### Added
- **Updates settings tab with channels.** Settings → Updates selects `Stable (main)` or
  `Development (dev)`, shows the current version, checks on demand, and installs through the
  existing verified-update path. The choice is stored in `update-channel`; unknown values are
  rejected rather than silently breaking the check.
- **Signed update feed (`appcast/<channel>.json`).** A release workflow step builds and signs
  the feed for the channel being published; `stable.json` is written only from `main`, so a
  dev build cannot reach a stable fleet. The client verifies an Ed25519 signature with a key
  baked into `res/betterdesk/client-update-signing.pub` before downloading anything, and then
  checks the artifact SHA-256. A build without the key fails closed.
- **Short-code pairing (`Connect with a code`).** Settings → Network takes a server address
  and a 6-character code instead of a long config string or a QR scan. The client probes
  HTTPS first (`443`, `:21114`, `:5443`), rejects a response that names a different host, and
  writes all endpoints only after a complete answer. `docs/CLIENT_API.md` documents the flow.
- **Server-connection diagnostics.** Settings → Network shows the resolved API base, its
  scheme, the measured latency and the last probe error, with "Test again" and "Reset".
- **System key store for secrets.** Access/device tokens and the unlock PIN move from the TOML
  config into Windows Credential Manager / macOS Keychain / Linux keyutils, with a one-time
  idempotent migration and a visible fallback (Settings → Network → "Secrets storage") when no
  store is available.
- **Docs:** `docs/UPDATE_CHANNELS.md` (feed format, key handling, troubleshooting) and
  `docs/BRANCHING.md` (`dev` vs `main`, what each push publishes).

### Changed
- **HTTPS is the default path to the API.** A server that only serves HTTPS on 443 is now
  reachable without typing a port. An explicit `api-server` is still used verbatim; plaintext
  is only accepted when the operator enables `allow-insecure-tls-fallback`.
- **Branding and enrollment run at startup**, not on the first heartbeat tick, so the home
  page and Settings → Branding are populated before the first `TIME_CONN` interval.

### Fixed
- **Windows service identity mismatch.** The MSI registered the service under the display
  name (`BetterDesk Client`) while the binary registered `BetterDesk-Client`, so stop,
  uninstall and upgrade could target a different service than the one running. The package now
  uses one `InstallName` for the service, HKCR key and config file, and removes a service left
  over from the old identity.
- **Unstable MSI product identity.** `UpgradeCode` was derived from `--app-name`, so any
  rename created a second product in ARP instead of upgrading, and the default revision
  version came from the wall clock, so two builds of the same source could force repairs.
  `UpgradeCode` is now a constant with the legacy codes listed as upgrade sources, and the
  revision is deterministic (`BETTERDESK_MSI_REVISION`).
- **Silent empty version.** `build.py` could produce `betterdesk--install.exe` when the
  version could not be parsed; it now fails instead.
- **Update download allowlist.** `get_update_download_file_from_url` accepted only
  `rustdesk/rustdesk`, so a valid BetterDesk manifest was rejected at download time. The
  trusted set is now an explicit allowlist that includes `UNITRONIX/BetterDesk-Client` and
  still refuses any other repository.
- **Update manifest URL.** The metadata URL was hardcoded to the upstream repository while
  release-tag validation already accepted this one; both now follow the release page.

### Notes
- `21116/tcp+udp` and `21117/tcp` must stay reachable: pairing configures the client but does
  not carry the session. See `BetterDesk/docs/CONNECTION_PORTS.md`.
- No configuration migration is required. Existing `api-server` values keep working unchanged.
