# BetterDesk Client — RUSTSEC advisory backlog

Status of the dependency advisories reported by `cargo audit` for this repo.

The client is pinned to **Rust 1.75** (see `flutter-build.yml`; macOS uses 1.81)
for Flutter/Windows ABI compatibility reasons inherited from upstream. Several
fixes require a newer compiler, so they are recorded here instead of being
silently ignored. `cargo audit` runs on every push and pull request
([`.github/workflows/client-security.yml`](../.github/workflows/client-security.yml)).

Last full audit: 2026-11-08, Rust 1.75.0, base `desktop/1.5.0`.

## Resolved in this pass

39 → 18 advisories through `Cargo.lock`-only updates, no source changes:

| Crate | From | To |
|---|---|---|
| bytes | 1.10.1 | 1.11.1 |
| crossbeam-channel | 0.5.13 | 0.5.15 |
| crossbeam-epoch | 0.9.18 | 0.9.20 |
| libz-sys | 1.1.18 | 1.1.31 |
| tracing-subscriber | 0.3.19 | 0.3.20 |
| webpki-roots | 1.0.9 | 1.0.10 |
| webpki-root-certs | 1.0.3 | 1.0.10 |
| zstd-safe | 7.1.0 | 7.4.0 |
| zstd-sys | 2.0.11 | 2.1.2 |
| rustls | 0.23.28 | 0.23.46 |
| rustls-webpki | 0.103.3 | 0.103.16 |
| openssl | 0.10.68 | 0.10.72 |
| quinn-proto | 0.11.13 | 0.11.15 |

Verified with `cargo build --lib` and `cargo test --lib` (123 passed).

## Remaining, blocked by the pinned toolchain

### quinn-proto 0.11.15 — 2 advisories

`RUSTSEC-2026-0339`, `RUSTSEC-2026-0340`.

Fixed in 0.11.17+, which pulls `rand 0.10` → `rand_pcg 0.10.2`, a crate whose
manifest uses **edition2024**. Cargo 1.75 refuses to parse it:

```
feature `edition2024` is required ... not stabilized in this version of Cargo (1.75.0)
```

0.11.15 is the highest patch that resolves and builds on 1.75.
**Needs:** a Rust toolchain bump (1.85+ for edition2024), which is a
cross-platform ABI change and requires an explicit maintainer decision.

### quick-xml 0.30 — 2 advisories

`RUSTSEC-2026-0194`, `RUSTSEC-2026-0195`. Fixed in 0.41, a breaking API change
for three dependent call sites. Not attempted here: it is a code change, not a
lockfile bump, and belongs in a dedicated PR with tests.

### time 0.1.45 — 2 advisories

`RUSTSEC-2020-0071`, `RUSTSEC-2026-0009`. Fixed in 0.3.47. The `time 0.1` line
is pulled in by a legacy transitive dependency and cannot be replaced without
changing that parent crate.

### crypto-bigint 0.5.5 — 1 advisory

`RUSTSEC-2026-0387` (timing variability). Fixed in 0.7.6, a semver-major bump
inside the crypto stack. Requires a coordinated `elliptic-curve`/`p256` update;
too risky to bundle with a backlog cleanup.

### idna 0.5.0 — 1 advisory

`RUSTSEC-2024-0421`. Fixed in 1.0.0, a semver-major update of a URL-handling
crate. Reachable through HTTP client dependencies; needs its own verification
pass because IDNA behaviour affects hostname matching.

### libgit2-sys 0.14.2 — 1 advisory

`RUSTSEC-2024-0013`. Build-only toolchain dependency (git operations during
build), not linked into the shipped client binary.

### ringbuf 0.3.3 — 1 advisory

`RUSTSEC-2026-0293`. Fixed in 0.5.2, a semver-major bump. Used by the audio
pipeline; needs runtime verification on all platforms.

### users 0.11.0 — 1 advisory

`RUSTSEC-2025-0040`. No patched version published. Linux-only user lookup.

## What is deliberately not done

- No toolchain bump. It changes the build ABI on Windows, Linux, macOS, ARM64
  and cannot be validated from one machine.
- No semver-major crypto/URL/audio dependency jumps bundled into a cleanup
  change. Those need their own PRs with runtime testing.

## How to re-check

```bash
cd BetterDesk-Client
cargo audit                 # full advisory list
cargo audit --json | jq '.vulnerabilities.count'
```
