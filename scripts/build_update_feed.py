#!/usr/bin/env python3
"""Build one BetterDesk client update feed (`appcast/<channel>.json` + `.sig`).

The client polls `appcast/<channel>.json` and verifies it against the Ed25519 key
embedded in `src/update_metadata.rs`. Signing reuses `generate_update_metadata.py` so the
wire format, validation and signature context have exactly one implementation.

Input is a directory of artifacts named `<platform>-<arch>.<ext>` as produced by
`betterdesk-desktop-release.yml` under `dist/clean/`.

Usage (CI):
    BETTERDESK_CLIENT_RELEASE_SIGNING_SEED=... \
    python scripts/build_update_feed.py \
        --artifacts-dir dist/clean \
        --channel dev \
        --version 1.5.0 \
        --release-id desktop-v1.5.0-dev.12-abcdef0 \
        --release-base-url https://github.com/UNITRONIX/BetterDesk-Client/releases/download \
        --published-at 2026-11-08T12:00:00Z \
        --out-dir dist/feed
"""

from __future__ import annotations

import argparse
import importlib.util
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GENERATOR = ROOT / "res" / "update" / "generate_update_metadata.py"

CHANNELS = ("dev", "stable")
APP_NAME = "betterdesk-client"

# (pattern, platform, arch group, format). Names must match the client's
# `current_update_platform/arch/format` values or the update is silently invisible.
ARTIFACT_PATTERNS = (
    (re.compile(r"^(?:betterdesk|rustdesk)-[0-9][0-9a-z._-]*-(?P<arch>x86_64|aarch64)\.msi$"), "windows", "msi"),
    (re.compile(r"^(?:betterdesk|rustdesk)-[0-9][0-9a-z._-]*-(?P<arch>x86_64|aarch64)\.exe$"), "windows", "exe"),
    (re.compile(r"^(?:betterdesk|rustdesk)-[0-9][0-9a-z._-]*-(?P<arch>x86_64|aarch64)\.dmg$"), "macos", "dmg"),
    (re.compile(r"^(?:betterdesk|rustdesk)-[0-9][0-9a-z._-]*-(?P<arch>x86_64|aarch64)\.deb$"), "linux", "deb"),
    (re.compile(r"^(?:betterdesk|rustdesk)-[0-9][0-9a-z._-]*-(?P<arch>x86_64|aarch64)\.rpm$"), "linux", "rpm"),
    (re.compile(r"^(?:betterdesk|rustdesk)-[0-9][0-9a-z._-]*-(?P<arch>x86_64|aarch64)\.AppImage$"), "linux", "appimage"),
)

# Formats the client can install unattended. Without one the feed is useless.
INSTALLABLE_FORMATS = {"exe", "msi", "dmg"}


def fail(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def load_generator():
    # `generate_update_metadata.py` imports its policy module by bare name, so put its
    # directory on sys.path the way running it directly would.
    generator_dir = str(GENERATOR.parent)
    if generator_dir not in sys.path:
        sys.path.insert(0, generator_dir)
    spec = importlib.util.spec_from_file_location("betterdesk_update_metadata", GENERATOR)
    if spec is None or spec.loader is None:
        fail(f"cannot load {GENERATOR}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def classify(path: Path) -> tuple[str, str, str] | None:
    for pattern, platform, file_format in ARTIFACT_PATTERNS:
        match = pattern.match(path.name)
        if match:
            return platform, match.group("arch"), file_format
    return None


def collect(artifacts_dir: Path) -> list[list[str]]:
    if not artifacts_dir.is_dir():
        fail(f"artifacts directory not found: {artifacts_dir}")
    specs: list[list[str]] = []
    seen: set[tuple[str, str, str]] = set()
    for path in sorted(artifacts_dir.iterdir()):
        if not path.is_file():
            continue
        classified = classify(path)
        if classified is None:
            continue
        selector = classified
        if selector in seen:
            fail(f"duplicate artifact selector {selector} for {path.name}")
        seen.add(selector)
        platform, arch, file_format = classified
        specs.append([platform, arch, file_format, str(path)])
    return specs


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--artifacts-dir", required=True, type=Path)
    parser.add_argument("--channel", required=True, choices=CHANNELS)
    parser.add_argument("--version", required=True)
    parser.add_argument("--release-id", required=True)
    parser.add_argument("--release-base-url", required=True)
    parser.add_argument("--published-at", required=True)
    parser.add_argument("--package-id", default="betterdesk-desktop")
    parser.add_argument("--out-dir", required=True, type=Path)
    parser.add_argument(
        "--require-installable",
        action="store_true",
        help="Fail when no exe/msi/dmg is present (feed the client cannot install).",
    )
    args = parser.parse_args()

    specs = collect(args.artifacts_dir)
    if not specs:
        fail(f"no artifact matching a known platform/arch/format under {args.artifacts_dir}")
    if args.require_installable and not any(spec[2] in INSTALLABLE_FORMATS for spec in specs):
        fail(
            "no installable artifact (exe/msi/dmg) in the feed; "
            "the client cannot auto-update from this release"
        )

    args.out_dir.mkdir(parents=True, exist_ok=True)
    metadata_out = args.out_dir / f"{args.channel}.json"
    signature_out = args.out_dir / f"{args.channel}.json.sig"

    generator = load_generator()
    sign_args = argparse.Namespace(
        artifact=specs,
        version=args.version,
        release_id=args.release_id,
        package_id=args.package_id,
        release_base_url=args.release_base_url,
        published_at=args.published_at,
        metadata_out=str(metadata_out),
        signature_out=str(signature_out),
        app_name=APP_NAME,
    )
    generator.command_sign(sign_args)

    # The channel is not part of the signed payload, so annotate next to it. Client UI
    # reads it from the file name; this sidecar exists for operators and the panel.
    (args.out_dir / f"{args.channel}.channel").write_text(args.channel + "\n", encoding="utf-8")

    print(f"wrote {metadata_out}")
    print(f"wrote {signature_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
