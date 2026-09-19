#!/usr/bin/env python3
"""Run a dependency-free contract test for Generator template packaging."""

from __future__ import annotations

import json
import shutil
import tempfile
from pathlib import Path

from pack_generator_templates import PLATFORMS, validate_manifest, pack_one


def main() -> int:
    with tempfile.TemporaryDirectory(prefix="betterdesk-generator-contract-") as value:
        root = Path(value)
        dist = root / "dist"
        out = root / "generator-templates"
        entries = []
        for platform, arch, folder in PLATFORMS:
            source = dist / folder
            if platform == "macos":
                binary = source / "BetterDesk.app" / "Contents" / "MacOS" / "betterdesk"
                binary.parent.mkdir(parents=True)
            else:
                binary = source / ("betterdesk.exe" if platform == "windows" else "betterdesk")
                source.mkdir(parents=True)
            binary.write_bytes(b"B" * (100 * 1024))
            destination = out / folder
            entries.append(pack_one(source, destination, platform, arch))

        manifest = {
            "schema_version": 2,
            "product": "betterdesk-desktop",
            "sku": "generator-templates",
            "version": "test",
            "required_platforms": [f"{platform}-{arch}" for platform, arch, _ in PLATFORMS],
            "templates": entries,
        }
        (out / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
        validate_manifest(out, manifest)
        assert not list(out.rglob("custom.txt"))
        assert (out / "windows-x86_64" / ".custom-txt-here").is_file()
        assert list((out / "macos-x86_64").rglob(".custom-txt-here"))
        shutil.rmtree(root)
    print("Generator contract: OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
