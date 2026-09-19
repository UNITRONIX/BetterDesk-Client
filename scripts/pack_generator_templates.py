#!/usr/bin/env python3
"""Pack BetterDesk desktop build trees into Generator templates.

Produces a directory (or tar.gz) the BetterDesk console can download and
patch with a signed custom.txt for Support Agent (incoming-only) builds.

Expected layout under --dist-root (per platform/arch):
  windows-x86_64/          # Release folder with betterdesk.exe (no custom.txt)
  windows-aarch64/
  linux-x86_64/            # extracted app tree or portable dir
  linux-aarch64/
  macos-x86_64/            # BetterDesk.app or Contents tree
  macos-aarch64/
  msi-template/            # optional prebuilt MSI templates from CI

Output:
  generator-templates/<platform>-<arch>/...
  generator-templates/manifest.json
  generator-templates-<version>.tar.gz (optional --archive)
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import tarfile
from pathlib import Path


PLATFORMS = (
    ("windows", "x86_64", "windows-x86_64"),
    ("windows", "aarch64", "windows-aarch64"),
    ("linux", "x86_64", "linux-x86_64"),
    ("linux", "aarch64", "linux-aarch64"),
    ("macos", "x86_64", "macos-x86_64"),
    ("macos", "aarch64", "macos-aarch64"),
)
PLATFORM_KEYS = tuple(f"{platform}-{arch}" for platform, arch, _ in PLATFORMS)
MIN_BINARY_SIZE = 32 * 1024


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def copy_tree(src: Path, dst: Path) -> None:
    if dst.exists():
        shutil.rmtree(dst)
    shutil.copytree(src, dst)


def ensure_no_custom_txt(root: Path) -> None:
    for p in root.rglob("custom.txt"):
        p.unlink()


def _relative_file(root: Path, names: tuple[str, ...]) -> str:
    for name in names:
        for match in sorted(root.rglob(name)):
            if match.is_file() and match.stat().st_size >= MIN_BINARY_SIZE:
                return match.relative_to(root).as_posix()
    return ""


def _validate_source(src: Path, platform: str) -> str:
    if platform == "macos":
        for app in sorted(src.rglob("*.app")):
            macos = app / "Contents" / "MacOS"
            if not macos.is_dir():
                continue
            binary = _relative_file(macos, ("betterdesk", "rustdesk"))
            if binary:
                return (macos / binary).relative_to(src).as_posix()
        raise ValueError(f"{src}: missing valid macOS .app bundle and desktop binary")

    binary = _relative_file(src, ("betterdesk.exe", "rustdesk.exe", "betterdesk", "rustdesk"))
    if not binary:
        raise ValueError(f"{src}: missing BetterDesk desktop binary")
    return binary


def _safe_archive_path(path: str) -> bool:
    candidate = Path(path)
    return not candidate.is_absolute() and ".." not in candidate.parts


def validate_manifest(root: Path, manifest: dict, require_all: bool = True) -> None:
    if manifest.get("schema_version") != 2:
        raise ValueError("manifest schema_version must be 2")
    if manifest.get("sku") != "generator-templates":
        raise ValueError("manifest sku must be generator-templates")
    templates = manifest.get("templates")
    if not isinstance(templates, list):
        raise ValueError("manifest templates must be a list")

    seen = set()
    for entry in templates:
        key = f"{entry.get('platform')}-{entry.get('arch')}"
        if entry.get("format") != "portable":
            continue
        if key in seen:
            raise ValueError(f"duplicate template: {key}")
        seen.add(key)
        template_path = entry.get("template_path")
        if not isinstance(template_path, str) or not _safe_archive_path(template_path):
            raise ValueError(f"invalid template path: {template_path!r}")
        template_root = root / template_path
        if not template_root.is_dir():
            raise ValueError(f"missing template directory: {template_path}")
        if any(path.is_symlink() for path in template_root.rglob("*")):
            raise ValueError(f"template contains symlink: {template_path}")
        if list(template_root.rglob("custom.txt")):
            raise ValueError(f"template contains custom.txt: {template_path}")
        if not list(template_root.rglob(".custom-txt-here")):
            raise ValueError(f"missing custom.txt marker: {template_path}")
        binary_path = entry.get("binary_path")
        if not isinstance(binary_path, str) or not _safe_archive_path(binary_path):
            raise ValueError(f"invalid binary path for {key}")
        binary = template_root / binary_path
        if not binary.is_file() or binary.stat().st_size < MIN_BINARY_SIZE:
            raise ValueError(f"missing or undersized binary for {key}")
        if not isinstance(entry.get("sha256"), str) or len(entry["sha256"]) != 64:
            raise ValueError(f"invalid template sha256 for {key}")
        archive = entry.get("archive")
        if not isinstance(archive, str) or not _safe_archive_path(archive):
            raise ValueError(f"invalid template archive for {key}")
        archive_path = root / archive
        if not archive_path.is_file() or sha256_file(archive_path) != entry["sha256"]:
            raise ValueError(f"template archive hash mismatch for {key}")

    required = set(PLATFORM_KEYS)
    declared = set(manifest.get("required_platforms", []))
    if declared != required:
        raise ValueError("manifest required_platforms does not match supported platforms")
    if require_all and seen != required:
        missing = ", ".join(sorted(required - seen))
        extra = ", ".join(sorted(seen - required))
        raise ValueError(f"portable platform set incomplete; missing={missing} extra={extra}")


def pack_one(src: Path, out_dir: Path, platform: str, arch: str) -> dict:
    binary_path = _validate_source(src, platform)
    out_dir.mkdir(parents=True, exist_ok=True)
    copy_tree(src, out_dir)
    ensure_no_custom_txt(out_dir)

    # Marker for console worker: where to place custom.txt
    if platform == "macos":
        app = next(out_dir.rglob("*.app"), None)
        if app is not None:
            target = app / "Contents" / "MacOS"
            target.mkdir(parents=True, exist_ok=True)
            (target / ".custom-txt-here").write_text("place custom.txt beside betterdesk binary\n")
            inject = str(Path("Contents") / "MacOS" / "custom.txt")
        else:
            (out_dir / ".custom-txt-here").write_text("place custom.txt in this directory\n")
            inject = "custom.txt"
    else:
        (out_dir / ".custom-txt-here").write_text("place custom.txt beside betterdesk binary\n")
        inject = "custom.txt"

    archive_name = f"{platform}-{arch}.tar.gz"
    archive_path = out_dir.parent / archive_name
    with tarfile.open(archive_path, "w:gz") as tar:
        tar.add(out_dir, arcname=f"{platform}-{arch}")

    return {
        "platform": platform,
        "arch": arch,
        "format": "portable",
        "path": f"{platform}-{arch}",
        "template_path": f"{platform}-{arch}",
        "archive": archive_name,
        "binary_path": binary_path,
        "inject_custom_txt": inject,
        "sha256": sha256_file(archive_path),
        "size": archive_path.stat().st_size,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist-root", type=Path, required=True, help="Root with per-platform build folders")
    parser.add_argument("--out", type=Path, required=True, help="Output directory for templates")
    parser.add_argument("--version", default=os.environ.get("VERSION", "0.0.0"))
    parser.add_argument("--archive", action="store_true", help="Also write generator-templates-<version>.tar.gz")
    parser.add_argument(
        "--allow-missing",
        action="store_true",
        help="Allow a partial platform set (for local development only)",
    )
    args = parser.parse_args()

    out = args.out
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)

    entries = []
    for platform, arch, folder in PLATFORMS:
        src = args.dist_root / folder
        if not src.is_dir():
            print(f"skip missing {src}")
            continue
        dest = out / f"{platform}-{arch}"
        entries.append(pack_one(src, dest, platform, arch))
        print(f"packed {platform}-{arch}")

    msi_src = args.dist_root / "msi-template"
    if msi_src.is_dir():
        msi_out = out / "msi-template"
        copy_tree(msi_src, msi_out)
        for msi in msi_out.glob("*.msi"):
            arch = "aarch64" if "aarch64" in msi.name else "x86_64"
            entries.append(
                {
                    "platform": "windows",
                    "arch": arch,
                    "format": "msi-template",
                    "path": f"msi-template/{msi.name}",
                    "inject_custom_txt": "cab2:custom.txt",
                    "sha256": sha256_file(msi),
                    "size": msi.stat().st_size,
                }
            )

    manifest = {
        "schema_version": 2,
        "product": "betterdesk-desktop",
        "sku": "generator-templates",
        "version": args.version,
        "required_platforms": list(PLATFORM_KEYS),
        "templates": entries,
    }
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    validate_manifest(out, manifest, require_all=not args.allow_missing)
    print(f"wrote {out / 'manifest.json'} ({len(entries)} templates)")

    if args.archive:
        archive = out.parent / f"generator-templates-{args.version}.tar.gz"
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(out, arcname="generator-templates")
        checksum = archive.with_name(archive.name + ".sha256")
        checksum.write_text(f"{sha256_file(archive)}  {archive.name}\n", encoding="utf-8")
        print(f"wrote {archive}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
