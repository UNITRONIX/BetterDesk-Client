#!/usr/bin/env python3
"""Compute and optionally apply the version used by a BetterDesk desktop build."""

from __future__ import annotations

import argparse
import re
from pathlib import Path

VERSION_RE = re.compile(r"^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$")


def read_base_version(root: Path) -> str:
    cargo = (root / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^version\s*=\s*"([^"]+)"', cargo, re.MULTILINE)
    if not match:
        raise SystemExit("Could not read version from Cargo.toml")
    return match.group(1)


def bump_patch(version: str) -> str:
    numbers = version.split("-", 1)[0].split(".")
    if len(numbers) != 3 or not all(part.isdigit() for part in numbers):
        raise SystemExit(f"Cannot bump non-release version: {version}")
    return f"{numbers[0]}.{numbers[1]}.{int(numbers[2]) + 1}"


def compute_version(root: Path, channel: str, run_number: int, requested: str) -> str:
    if requested:
        version = requested.strip()
    elif channel == "stable":
        version = bump_patch(read_base_version(root))
    else:
        base = read_base_version(root).split("-", 1)[0]
        version = f"{base}-{run_number}" if run_number > 0 else base
    if not VERSION_RE.fullmatch(version):
        raise SystemExit(f"Invalid release version: {version}")
    return version


def apply_version(root: Path, version: str, build_number: int) -> None:
    cargo_path = root / "Cargo.toml"
    cargo = cargo_path.read_text(encoding="utf-8")
    updated, count = re.subn(
        r'(^version\s*=\s*)"[^"]+"',
        rf'\1"{version}"',
        cargo,
        count=1,
        flags=re.MULTILINE,
    )
    if count != 1:
        raise SystemExit("Could not update version in Cargo.toml")
    cargo_path.write_text(updated, encoding="utf-8")

    # Cargo.lock records this package's own version too, and every build here
    # passes --locked, which refuses to touch a stale lock. Rewrite just that
    # entry so the lock stays valid without re-resolving the whole graph.
    lock_path = root / "Cargo.lock"
    if lock_path.is_file():
        lock = lock_path.read_text(encoding="utf-8")
        # The workspace member block is `name = "rustdesk"` followed by a
        # version line, with no `source` (local path dependencies have none).
        updated_lock, lock_count = re.subn(
            r'(\[\[package\]\]\nname = "rustdesk"\nversion = ")[^"]+(")',
            rf'\g<1>{version}\g<2>',
            lock,
            count=1,
        )
        if lock_count == 1:
            lock_path.write_text(updated_lock, encoding="utf-8")

    pubspec_path = root / "flutter" / "pubspec.yaml"
    pubspec = pubspec_path.read_text(encoding="utf-8")
    existing = re.search(r"^version:\s*[^\s+]+(?:\+(\d+))?", pubspec, re.MULTILINE)
    if not existing:
        raise SystemExit("Could not update version in flutter/pubspec.yaml")
    number = build_number if build_number > 0 else int(existing.group(1) or 0)
    replacement = f"version: {version}+{number}"
    updated, count = re.subn(
        r"^version:\s*[^\s]+",
        replacement,
        pubspec,
        count=1,
        flags=re.MULTILINE,
    )
    if count != 1:
        raise SystemExit("Could not update version in flutter/pubspec.yaml")
    pubspec_path.write_text(updated, encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--channel", choices=("dev", "stable"), default="dev")
    parser.add_argument("--run-number", type=int, default=0)
    parser.add_argument("--version", default="")
    parser.add_argument("--build-number", type=int, default=0)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    version = compute_version(root, args.channel, args.run_number, args.version)
    if args.apply:
        apply_version(root, version, args.build_number or args.run_number)
    print(version)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
