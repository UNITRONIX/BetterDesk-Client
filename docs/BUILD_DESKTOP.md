# Desktop client build (Windows + Linux)

Guide for compiling the **Flutter desktop** client on **Windows x64** and **Linux x64**.
Tool versions match CI in [`.github/workflows/flutter-build.yml`](../.github/workflows/flutter-build.yml).

Mobile / web / macOS / ARM are out of scope here; leave those trees in the repo.

## Reference versions (CI)

| Tool | Version |
|------|---------|
| Flutter (desktop x64) | **3.24.5** |
| Rust | **1.75** |
| LLVM / Clang | **15.0.6** |
| vcpkg commit / `vcpkg.json` baseline | **`9e593bb18ea69cc5095e012465dcd675a822ed0d`** |
| flutter_rust_bridge (regenerate only) | **1.80.1** |
| Python | 3.x (`build.py`) |

## Submodule (`libs/hbb_common`)

`libs/hbb_common` is a **git submodule**. Without it, Cargo cannot build.

```sh
git submodule update --init --recursive
```

Verify: `libs/hbb_common/Cargo.toml` and `libs/hbb_common/src/config.rs` exist.

Environment check scripts also verify this (see below).

## Environment check / activate scripts

From the repo root:

```powershell
# Windows — load tools into the current session (optional -Persist writes User env)
. .\scripts\activate_desktop_env_windows.ps1
# or persist:
. .\scripts\activate_desktop_env_windows.ps1 -Persist

# Windows — verify
powershell -ExecutionPolicy Bypass -File .\scripts\check_desktop_env_windows.ps1
```

```sh
# Linux
chmod +x scripts/check_desktop_env_linux.sh
./scripts/check_desktop_env_linux.sh
```

Check scripts only **verify**; they do not install tools. On this machine the reference layout is:

The installer/launcher contract can be checked without a desktop toolchain:

```sh
python3 scripts/validate_desktop_packaging.py
```

| Tool | Path |
|------|------|
| Flutter 3.24.5 | `C:\tools\flutter-3.24.5` |
| vcpkg (CI commit) | `C:\tools\vcpkg` |
| LLVM 15.0.6 | `C:\tools\LLVM-15.0.6` |

---

## Windows x64

### Prerequisites

1. **Visual Studio 2022** — workload *Desktop development with C++* (MSVC + Windows SDK).
2. **Rust 1.75**
   ```bat
   rustup toolchain install 1.75
   rustup default 1.75
   rustup target add x86_64-pc-windows-msvc
   ```
3. **Flutter 3.24.5** (stable)
   ```bat
   flutter config --enable-windows-desktop
   flutter doctor
   ```
4. **LLVM 15.0.6** — set `LIBCLANG_PATH` to the LLVM `bin` directory (needed by bindgen).
5. **vcpkg** pinned to the CI commit:
   ```bat
   git clone https://github.com/microsoft/vcpkg C:\vcpkg
   cd C:\vcpkg
   git checkout 9e593bb18ea69cc5095e012465dcd675a822ed0d
   bootstrap-vcpkg.bat
   setx VCPKG_ROOT C:\vcpkg
   ```
   Restart the shell so `VCPKG_ROOT` is visible, then from the **repo root**:
   ```bat
   %VCPKG_ROOT%\vcpkg install --triplet x64-windows-static --x-install-root=%VCPKG_ROOT%\installed
   ```
   This uses [`vcpkg.json`](../vcpkg.json) and overlays under [`res/vcpkg`](../res/vcpkg).
6. **Python 3** and **Git**.

### Optional (match CI Flutter engine)

For closer parity with upstream Windows CI:

1. Download [windows-x64-release.zip](https://github.com/rustdesk/engine/releases/download/main/windows-x64-release.zip) and replace files under Flutter’s `bin/cache/artifacts/engine/windows-x64-release/`.
2. Apply [`.github/patches/flutter_3.24.4_dropdown_menu_enableFilter.diff`](../.github/patches/flutter_3.24.4_dropdown_menu_enableFilter.diff) inside the Flutter SDK tree.

### Smoke-build (Windows)

```bat
git submodule update --init --recursive
python build.py --flutter --hwcodec --vram --skip-portable-pack
```

If `--vram` fails (missing GPU / Media SDK pieces), use:

```bat
python build.py --flutter --hwcodec --skip-portable-pack
```

**Artifact:** `flutter/build/windows/x64/runner/Release/betterdesk.exe` (+ `librustdesk.dll`)

---

## Linux x64

Target: **Ubuntu 22.04+ / Debian** (native or WSL2).

### System packages

```sh
sudo apt-get update
sudo apt-get install -y \
  build-essential clang cmake curl gcc g++ git \
  nasm ninja-build pkg-config python3 unzip wget xz-utils \
  libgtk-3-dev libayatana-appindicator3-dev \
  libasound2-dev libpulse-dev libva-dev \
  libclang-dev llvm-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libxcb-randr0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libxdo-dev libxfixes-dev libssl-dev
# Avoid conflict with vcpkg opus:
sudo apt-get remove -y libopus-dev || true
```

### Tooling

1. Rust **1.75**, target `x86_64-unknown-linux-gnu`.
2. Flutter **3.24.5**, then `flutter config --enable-linux-desktop`.
3. vcpkg on commit `9e593bb18ea69cc5095e012465dcd675a822ed0d`:
   ```sh
   export VCPKG_ROOT="$HOME/vcpkg"
   git clone https://github.com/microsoft/vcpkg "$VCPKG_ROOT"
   cd "$VCPKG_ROOT"
   git checkout 9e593bb18ea69cc5095e012465dcd675a822ed0d
   ./bootstrap-vcpkg.sh
   cd /path/to/BetterDesk-Client
   "$VCPKG_ROOT/vcpkg" install --triplet x64-linux --x-install-root="$VCPKG_ROOT/installed"
   ```
4. CMake 3.x+ (if vcpkg fails on SPDX scripts, install CMake **4.3.0** as in CI `VCPKG_CMAKE_VERSION`).

Set `LIBCLANG_PATH` if bindgen cannot find libclang (often `/usr/lib/llvm-15/lib` or similar).

### Smoke-build (Linux)

```sh
git submodule update --init --recursive
python3 ./build.py --flutter --hwcodec
```

**Artifact:** `flutter/build/linux/x64/release/bundle/`

`build.py` may also produce a `.deb` in the repo root when packaging succeeds.

---

## Environment variables

| Variable | Purpose |
|----------|---------|
| `VCPKG_ROOT` | Root of the pinned vcpkg checkout (required) |
| `LIBCLANG_PATH` | Directory containing `libclang` / `libclang.dll` (bindgen) |
| `CARGO_TARGET_DIR` | Prefer repo `target/` (set by `scripts/activate_desktop_env_windows.ps1` / `build.py`) so `librustdesk.dll` lands next to Flutter |
| `PATH` | Must include `rustc`/`cargo`, `flutter`, `python`/`python3` |

---

## Common failures

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| Missing `hbb_common` / path errors in Cargo | Submodule not initialized | `git submodule update --init --recursive` |
| vcpkg / ffmpeg / libvpx link errors | Wrong triplet or unset `VCPKG_ROOT` | Windows: `x64-windows-static`; Linux: `x64-linux`; reinstall from repo root |
| `unable to find libclang` | LLVM not installed / `LIBCLANG_PATH` unset | Install LLVM 15 and point `LIBCLANG_PATH` at its `bin` (Windows) or lib dir (Linux) |
| Opus / duplicate symbols on Linux | System `libopus-dev` clashes with vcpkg | `sudo apt-get remove -y libopus-dev` |
| Flutter / Dart version skew | Not on 3.24.5 | Install Flutter 3.24.5 to match CI |
| Rust edition / dependency failures | Wrong rustc | Use toolchain **1.75** |
| Windows C++ compile failures | No MSVC / Windows SDK | Install VS 2022 C++ desktop workload |

---

## What this guide does not cover

- Android / iOS / macOS / ARM builds
- Automatic installation of Visual Studio or Flutter (install those manually, then use the check scripts)

Official BetterDesk identity, public-server kill, Client Generator bake-in (`custom.txt`), Support Agent (incoming-only), and clean desktop CI: [OFFICIAL_CLIENT.md](OFFICIAL_CLIENT.md).

Example configs: [`examples/betterdesk-custom.example.json`](../examples/betterdesk-custom.example.json), [`examples/betterdesk-support-agent.example.json`](../examples/betterdesk-support-agent.example.json).

Release workflow (clean desktop agents + installers + generator templates): [`.github/workflows/betterdesk-desktop-release.yml`](../.github/workflows/betterdesk-desktop-release.yml). Desktop-only reusable build: [`.github/workflows/flutter-build.yml`](../.github/workflows/flutter-build.yml).

## Generator template validation

The release workflow publishes clean templates only. The Support Agent is created later
by the BetterDesk console with a signed `custom.txt`; release builds reject unsigned
custom configuration. To build and validate a local template package:

```sh
python scripts/pack_generator_templates.py \
  --dist-root ./dist \
  --out ./generator-templates \
  --version 1.5.0 \
  --archive
python scripts/validate_generator_templates.py --root ./generator-templates
```

The package must contain Windows, Linux and macOS x86_64/aarch64 templates, a valid
binary and a safe `.custom-txt-here` injection marker for each target. The console
verifies the release archive checksum before installing it.
