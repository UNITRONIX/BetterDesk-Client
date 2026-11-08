# Branching and releases

| Branch | Role | CI |
|--------|------|----|
| **`dev`** | Default. All work lands here. | Builds a prerelease on every push, publishes `appcast/dev.json`. |
| **`main`** | Stable only. | Builds a stable release, publishes `appcast/stable.json`. |
| `update-feed` | Machine-written. Holds the signed update feeds. | Written by the release workflow; never edit by hand. |

## Rules

1. **Work on `dev`.** Feature branches branch off `dev` and merge back into `dev`.
2. **`main` only for a release.** A `dev` → `main` merge is the release act: it is what
   promotes builds to the `stable` update channel. Do not push ordinary fixes to `main`.
3. **Never edit `update-feed`.** It is force-pushed by CI; a manual commit is overwritten and
   would publish a feed that does not match any release.
4. **Tags are `desktop-v<version>`** (stable) and `desktop-v<version>-dev.<run>-<sha>` (dev).
   No `/` in a tag — a slash breaks the release asset URL the client validates.

## What a push produces

| Push to | Tag | Release | Feed | Client channel affected |
|---------|-----|---------|------|-------------------------|
| `dev` | `desktop-v1.5.0-dev.<run>-<sha>` | prerelease | `appcast/dev.json` | clients on `dev` |
| `main` | `desktop-v1.5.0` | release (latest) | `appcast/stable.json` | clients on `stable` |

A `dev` build can never reach a `stable` client: `stable.json` is written only from `main`.

## Manual runs

`workflow_dispatch` takes `tag_name` and `channel`. Choose `channel: dev` for a test build
(published as a prerelease and to `dev.json`) or `channel: stable` for a real release.

## Verifying a release

```bash
# The feed the client will actually read
curl -s https://raw.githubusercontent.com/UNITRONIX/BetterDesk-Client/update-feed/appcast/stable.json | head -30

# The signature next to it
curl -s https://raw.githubusercontent.com/UNITRONIX/BetterDesk-Client/update-feed/appcast/stable.json.sig
```

Then on a client: Settings → Updates → **Check now** for the matching channel.

See [UPDATE_CHANNELS.md](UPDATE_CHANNELS.md) for the feed format and signing key.
