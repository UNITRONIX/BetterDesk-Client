# Update channels (`stable` / `dev`)

The client discovers updates from a signed feed, `appcast/<channel>.json`. It never calls
rustdesk.com and never scrapes a release page.

| Channel | Source branch | Feed | Client default |
|---------|---------------|------|----------------|
| `stable` | `main` | `appcast/stable.json` | Release builds |
| `dev` | `dev` | `appcast/dev.json` | Debug builds, and any install where the operator switched the channel |

`stable.json` is written **only** by a push to `main`. A development build can therefore
never reach a fleet that follows `stable`, even if someone publishes it as a release.

## Where the feed lives

```
https://raw.githubusercontent.com/UNITRONIX/BetterDesk-Client/update-feed/appcast/stable.json
https://raw.githubusercontent.com/UNITRONIX/BetterDesk-Client/update-feed/appcast/dev.json
+ .sig next to each
```

The branch is `update-feed`; the release workflow force-updates it on every `dev` and `main`
build. Artifacts themselves stay on the GitHub Release, so a feed is a few hundred bytes.

## Signed, then verified

The feed is signed with Ed25519 (`scripts/build_update_feed.py` →
`res/update/generate_update_metadata.py`) and verified against a key baked into
`res/betterdesk/client-update-signing.pub`.

```bash
# One-time: generate the key pair. Store the seed as the repository secret
# BETTERDESK_CLIENT_RELEASE_SIGNING_SEED and rebuild the client so the .pub is embedded.
python scripts/generate_custom_client_signing_key.py --kind update
```

Properties the signature gives:

- The feed cannot be altered by the CDN, an ISP, or a compromised raw.githubusercontent path.
- A feed signed for upstream RustDesk is rejected: BetterDesk uses a different app id, key id
  and signature context (`betterdesk-client`, `2026-ed25519-betterdesk-client`,
  `BetterDesk client update metadata v1`).
- A build with no embedded key **fails closed**: Settings → Updates reports that the feed
  cannot be authenticated instead of installing anything.

After the signature, the artifact is downloaded and its SHA-256 checked against the manifest
before the installer runs (`src/updater/download.rs`).

## Switching the channel

Settings → **Updates** → radio selection. The value is stored in `update-channel` and only
`stable` / `main` / `dev` / `development` are accepted; an unknown value would silently break
every check, so it is rejected rather than stored.

Changing the channel triggers an immediate check; checking manually bypasses the 10-minute
minimum interval but still runs off the UI thread.

## What a feed contains

```json
{
  "schema_version": 1,
  "app": "betterdesk-client",
  "package_id": "betterdesk-desktop",
  "version": "1.5.1",
  "release_id": "desktop-v1.5.1",
  "published_at": "2026-11-08T12:00:00Z",
  "signature_key_id": "2026-ed25519-betterdesk-client",
  "artifacts": [
    {
      "platform": "windows", "arch": "x86_64", "format": "msi",
      "url": "https://github.com/UNITRONIX/BetterDesk-Client/releases/download/desktop-v1.5.1/betterdesk-1.5.1-x86_64.msi",
      "file_name": "betterdesk-1.5.1-x86_64.msi",
      "size": 61234567,
      "sha256": "…"
    }
  ]
}
```

`platform` / `arch` / `format` must match what the client asks for, which is why the builder
classifies artifact names instead of trusting a directory listing: an unrecognised name is
skipped (or fails the build with `--require-installable`), never silently published as an
update nobody can install.

## Release tags

Tags are `desktop-v<version>` (stable) and `desktop-v<version>-dev.<run>-<sha>` (dev).
**No `/` in the tag**: a slash splits the release asset URL into extra path segments, which
the client's release-id validation rejects by design. Older `desktop/<version>` tags are
still accepted for reading a release page but are not produced any more.

## Troubleshooting

| Symptom | Cause | Check |
|---------|-------|-------|
| "You are up to date" on an old build | Feed not updated, or version comparison sees the same version | `curl -s …/appcast/stable.json | head -20` |
| "Update check failed: … 404" | Feed branch missing, or the workflow never published for this channel | Does `update-feed` exist? Does it contain `appcast/stable.json`? |
| "…cannot be authenticated" | Client built without `client-update-signing.pub`, or the seed does not match | `python res/update/generate_update_metadata.py check-key` |
| Feed has no artifact for the platform | Release did not build that target, or the name does not match the classifier | Inspect `artifacts[]` in the feed |
| Install runs but nothing changes | MSI upgrade blocked by a stale product identity | See `docs/OFFICIAL_CLIENT.md` and the installer identity section of `CHANGELOG.md` |
