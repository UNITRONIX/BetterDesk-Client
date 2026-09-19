# BetterDesk Client

<div align="center">

<img src="res/betterdesk.png" alt="BetterDesk" width="280">

<br><br>

![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)
![Flutter](https://img.shields.io/badge/Flutter-desktop-02569B.svg)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-lightgrey.svg)

[![Sponsor](https://img.shields.io/badge/GitHub-Sponsor-181717?logo=github&logoColor=white&style=flat)](https://github.com/sponsors/UNITRONIX)
[![Discord](https://img.shields.io/badge/Discord-Join%20us-5865F2?logo=discord&logoColor=white&style=flat)](https://discord.gg/MPp9hyyG97)

**Official UNITRONIX desktop remote client** — a fork of [RustDesk](https://github.com/rustdesk/rustdesk), licensed under **AGPL-3.0**.

[Build](docs/BUILD_DESKTOP.md) · [Official client](docs/OFFICIAL_CLIENT.md) · [Connectivity](docs/CONNECTIVITY.md) · [Client API](docs/CLIENT_API.md) · [Upstream](https://github.com/rustdesk/rustdesk)

</div>

---

> [!Caution]
> **Misuse Disclaimer:** The developers of this software do not condone or support any unethical or illegal use. Misuse, such as unauthorized access, control or invasion of privacy, is strictly against our guidelines. The authors are not responsible for any misuse of the application.

This repository is the **official BetterDesk desktop client** ([UNITRONIX/BetterDesk-Client](https://github.com/UNITRONIX/BetterDesk-Client)). Active work targets **Flutter desktop on Windows and Linux**. Mobile and web trees remain in-repo but are not the current focus.

> **AI-assisted development:** BetterDesk Client is created and maintained with the assistance of artificial intelligence tools.

The client **does not** connect to public `*.rustdesk.com` infrastructure. Configure your own ID / Relay / API / Key (Settings → Network), import a deploy string from the BetterDesk panel, or use a Generator bake-in (`custom.txt`). Without a configured server, the client does not register on a public cloud.

Pair it with the self-hosted BetterDesk server and web console: [UNITRONIX/BetterDesk](https://github.com/UNITRONIX/BetterDesk).

---

## Product SKUs

| SKU | Role |
|-----|------|
| **BetterDesk Client** | Full desktop client (outbound + inbound) |
| **BetterDesk Support Agent** | Incoming-only agent (Generator bake-in; no outbound remote control) |

Both identify to the API as `betterdesk-desktop` with a distinct `product_sku`. Details: [docs/OFFICIAL_CLIENT.md](docs/OFFICIAL_CLIENT.md).

---

## Documentation

| Topic | Doc |
|-------|-----|
| Official client behaviour (no public cloud, branding, Generator, Support Agent) | [docs/OFFICIAL_CLIENT.md](docs/OFFICIAL_CLIENT.md) |
| Desktop build (Windows / Linux Flutter) | [docs/BUILD_DESKTOP.md](docs/BUILD_DESKTOP.md) |
| How connections work (P2P / LAN / relay / API) | [docs/CONNECTIVITY.md](docs/CONNECTIVITY.md) |
| Client API layers (HTTP / FFI / IPC) | [docs/CLIENT_API.md](docs/CLIENT_API.md) |
| BetterDesk server wiki | [BetterDesk Wiki](https://github.com/UNITRONIX/BetterDesk/wiki) |

---

## Build (desktop)

Full steps, tool versions, and smoke-build commands: **[docs/BUILD_DESKTOP.md](docs/BUILD_DESKTOP.md)**.

Short checklist:

1. Init the required submodule:
   ```sh
   git submodule update --init --recursive
   ```
2. Install the toolchain pinned in that guide (Flutter **3.24.5**, Rust **1.75**, vcpkg, LLVM/Clang as needed).
3. On Windows, optionally activate/check the env:
   ```powershell
   . .\scripts\activate_desktop_env_windows.ps1
   powershell -ExecutionPolicy Bypass -File .\scripts\check_desktop_env_windows.ps1
   ```
4. Build the Flutter desktop app for Windows or Linux per `BUILD_DESKTOP.md`.

---

## Repository layout

- **`src/`** — Rust application (server services, client connection, platform code)
- **`src/server/`** — audio / clipboard / input / video / network services
- **`src/platform/`** — platform-specific code
- **`flutter/`** — Flutter UI (desktop under `flutter/lib/desktop/`)
- **`libs/hbb_common/`** — config, protobuf, shared utilities (git submodule)
- **`libs/scrap/`** — screen capture
- **`libs/enigo/`** — input control
- **`libs/clipboard/`** — clipboard / file copy-paste
- **`res/betterdesk/`** — Generator signing keys and notes for `custom.txt`

---

## Attribution and license

BetterDesk Client is a fork of [RustDesk](https://github.com/rustdesk/rustdesk). The About UI and sysinfo keep upstream attribution and source links as required by the project policy ([docs/OFFICIAL_CLIENT.md](docs/OFFICIAL_CLIENT.md)).

[AGPL-3.0](LICENSE) · Source: [UNITRONIX/BetterDesk-Client](https://github.com/UNITRONIX/BetterDesk-Client) · Upstream: [rustdesk/rustdesk](https://github.com/rustdesk/rustdesk)

---

## Support

- [Discord](https://discord.gg/MPp9hyyG97)
- [BetterDesk Issues](https://github.com/UNITRONIX/BetterDesk/issues)
- [BetterDesk-Client Issues](https://github.com/UNITRONIX/BetterDesk-Client/issues)
