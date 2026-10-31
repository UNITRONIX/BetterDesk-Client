#!/usr/bin/env python3
"""Read and bump the canonical BetterDesk Client version."""

from __future__ import annotations

import argparse
import re
from pathlib import Path


SEMVER_RE = re.compile(r"^(\d+)\.(\d+)\.(\d+)$")
ROOT = Path(__file__).resolve().parents[1]
VERSION_PATH = ROOT / "VERSION"


def parse_version(value: str) -> tuple[int, int, int]:
    match = SEMVER_RE.fullmatch(value.strip())
    if not match:
        raise ValueError(f"invalid semantic version: {value!r}")
    major, minor, patch = (int(part) for part in match.groups())
    if patch > 100:
        raise ValueError(f"patch version exceeds 100: {value!r}")
    return major, minor, patch


def format_version(parts: tuple[int, int, int]) -> str:
    return ".".join(str(part) for part in parts)


def next_patch_version(value: str) -> str:
    major, minor, patch = parse_version(value)
    if patch == 100:
        return format_version((major, minor + 1, 0))
    return format_version((major, minor, patch + 1))


def canonical_version() -> str:
    return VERSION_PATH.read_text(encoding="utf-8").strip()


def update_manifests(version: str) -> list[Path]:
    changed: list[Path] = []
    updates: list[tuple[Path, str, str]] = []

    cargo_path = ROOT / "Cargo.toml"
    cargo = cargo_path.read_text(encoding="utf-8")
    updated_cargo, count = re.subn(
        r'(?m)^(version\s*=\s*)"[^"]+"$',
        rf'\1"{version}"',
        cargo,
        count=1,
    )
    if count != 1:
        raise ValueError("Cargo.toml package version not found")
    updates.append((cargo_path, cargo, updated_cargo))

    pubspec_path = ROOT / "flutter" / "pubspec.yaml"
    pubspec = pubspec_path.read_text(encoding="utf-8")
    updated_pubspec, count = re.subn(
        r"(?m)^(version:\s*)\d+\.\d+\.\d+(\+[^\s]+)?$",
        rf"\g<1>{version}\g<2>",
        pubspec,
        count=1,
    )
    if count != 1:
        raise ValueError("flutter/pubspec.yaml version not found")
    updates.append((pubspec_path, pubspec, updated_pubspec))

    for path, original, updated in updates:
        if updated != original:
            path.write_text(updated, encoding="utf-8")
            changed.append(path)

    return changed


def bump() -> str:
    current = canonical_version()
    next_version = next_patch_version(current)
    VERSION_PATH.write_text(f"{next_version}\n", encoding="utf-8")
    update_manifests(next_version)
    return next_version


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--patch", action="store_true", help="increment the patch version")
    parser.add_argument("--verify", action="store_true", help="validate manifests against VERSION")
    args = parser.parse_args()

    if args.patch == args.verify:
        parser.error("choose exactly one of --patch or --verify")

    version = canonical_version()
    parse_version(version)
    if args.verify:
        expected = version
        cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
        pubspec = (ROOT / "flutter" / "pubspec.yaml").read_text(encoding="utf-8")
        cargo_version = re.search(r'(?m)^version\s*=\s*"([^"]+)"$', cargo)
        pubspec_version = re.search(r"(?m)^version:\s*([^\s+]+)", pubspec)
        actual = {
            "Cargo.toml": cargo_version.group(1) if cargo_version else None,
            "flutter/pubspec.yaml": pubspec_version.group(1) if pubspec_version else None,
        }
        mismatches = [f"{name}={value}" for name, value in actual.items() if value != expected]
        if mismatches:
            raise SystemExit("Version mismatch: " + ", ".join(mismatches))
        print(f"Client version: OK ({expected})")
        return 0

    next_version = bump()
    print(next_version)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
