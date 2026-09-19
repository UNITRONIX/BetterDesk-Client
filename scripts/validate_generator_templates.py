#!/usr/bin/env python3
"""Validate an extracted BetterDesk Generator template package."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from pack_generator_templates import validate_manifest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, required=True, help="Extracted generator-templates directory")
    parser.add_argument("--allow-missing", action="store_true", help="Allow a partial platform set")
    args = parser.parse_args()

    root = args.root.resolve()
    manifest_path = root / "manifest.json"
    if not manifest_path.is_file():
        raise SystemExit(f"missing manifest: {manifest_path}")
    try:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        validate_manifest(root, manifest, require_all=not args.allow_missing)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        raise SystemExit(f"generator template validation failed: {exc}") from exc

    print(
        "Generator template package: OK "
        f"(version={manifest.get('version')}, templates={len(manifest.get('templates', []))})"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
