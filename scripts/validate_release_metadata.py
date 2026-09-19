#!/usr/bin/env python3
"""Validate that desktop release metadata uses one version."""

from __future__ import annotations

import argparse
import re
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]

    cargo = (root / "Cargo.toml").read_text(encoding="utf-8")
    pubspec = (root / "flutter" / "pubspec.yaml").read_text(encoding="utf-8")
    cargo_version = re.search(r'^version\s*=\s*"([^"]+)"', cargo, re.MULTILINE)
    flutter_version = re.search(r"^version:\s*([^\s+]+)", pubspec, re.MULTILINE)
    if not cargo_version or not flutter_version:
        raise SystemExit("Could not read Cargo.toml or flutter/pubspec.yaml version")

    expected = args.version.strip()
    actual = {
        "Cargo.toml": cargo_version.group(1),
        "flutter/pubspec.yaml": flutter_version.group(1),
    }
    mismatches = [f"{name}={value}" for name, value in actual.items() if value != expected]
    if mismatches:
        raise SystemExit(f"Release version mismatch: expected {expected}; " + ", ".join(mismatches))
    print(f"Release metadata: OK ({expected})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
