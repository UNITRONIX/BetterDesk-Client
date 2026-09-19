# BetterDesk `custom.txt` signing

Used by the official desktop client and the web panel **Support Generator**.

## Files

| File | Purpose |
|------|---------|
| `custom-client-signing.pub` | NaCl/Ed25519 public key embedded in the client (`read_custom_client`) |
| `custom-client-signing.seed` | **Private** 32-byte seed (base64). Create with the script; do not publish |
| `custom-client-signing.seed.example` | Lab/RFC test seed matching the committed `.pub` — **replace before production** |

## Generate keys

```bash
python scripts/generate_custom_client_signing_key.py
```

Requires `pynacl`. After generation, rebuild the client so `include_str!` picks up the new `.pub`.

**Production:** do not ship the example seed. Generate a fresh pair, embed the new `.pub` in Client releases, and store the `.seed` only on the BetterDesk console Generator module (`data/modules/betterdesk-support-generator/`).

## `custom.txt` formats

1. **Plain JSON** — accepted only by debug builds for local development. Release
   clients reject unsigned `custom.txt`:

```json
{
  "app-name": "BetterDesk",
  "default-settings": {
    "custom-rendezvous-server": "desk.example.com",
    "relay-server": "desk.example.com",
    "api-server": "http://desk.example.com:21114",
    "key": "<id_ed25519.pub>"
  }
}
```

2. **Signed blob** (production) — base64(NaCl-sign(JSON)), verified with `.pub`.

Server options belong under `default-settings` (user can change) or `override-settings` (locked for fleet builds).

Top-level string keys (e.g. `"conn-type": "incoming"`, `"disable-settings": "Y"`) go into `HARD_SETTINGS`.

### Support Agent (incoming-only)

- Example: [`examples/betterdesk-support-agent.example.json`](../../examples/betterdesk-support-agent.example.json)
- Docs: [OFFICIAL_CLIENT.md](../../docs/OFFICIAL_CLIENT.md)

```bash
cp examples/betterdesk-support-agent.example.json /path/to/Release/custom.txt
python scripts/sign_custom_client_config.py examples/betterdesk-support-agent.example.json > custom.txt
```

Runtime branding: `GET /api/branding` (both SKUs). Enrollment: `POST /api/devices/register` (`device_type=betterdesk-desktop` + `product_sku` / `conn_mode`).

### Generator templates (CI)

[`.github/workflows/betterdesk-desktop-release.yml`](../../.github/workflows/betterdesk-desktop-release.yml) publishes clean desktops + `generator-templates-<version>.tar.gz`.

```bash
python scripts/pack_generator_templates.py --dist-root ./dist --out ./generator-templates --version 1.5.0 --archive
```

The BetterDesk console installs the archive from a Client release or accepts a
local upload. It verifies the archive checksum, manifest schema, platform
completeness, binary paths and injection markers before replacing an existing
module. Production builds require `BETTERDESK_CUSTOM_CLIENT_SIGNING_SEED` or
`data/custom-client-signing.seed`; the seed is never part of the Client release.
