#!/usr/bin/env python3
"""Regression tests for the release download table.

Reproduces the 1.5.0 / dev-23 releases: the MSI cell linked `betterdesk-template-<arch>.msi`
(the console Generator patcher template) instead of the real installer, because "template"
matched the generic `<name>-<arch>.msi` pattern and sorted after the real file.

Run: python scripts/test_release_download_table.py
"""

from __future__ import annotations

import importlib.util
import re
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def load_generator():
    spec = importlib.util.spec_from_file_location(
        "gen", ROOT / "scripts" / "generate_release_download_table.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


gen = load_generator()

# Exactly the asset set published for desktop/1.5.0-dev.23 and desktop/1.5.0.
REAL_RELEASE_ASSETS = [
    "betterdesk-1.5.0-aarch64.exe",
    "betterdesk-1.5.0-aarch64.msi",
    "betterdesk-1.5.0-generator-manifest.json",
    "betterdesk-1.5.0-linux-aarch64-portable.tar.gz",
    "betterdesk-1.5.0-linux-aarch64-suse.rpm",
    "betterdesk-1.5.0-linux-aarch64.deb",
    "betterdesk-1.5.0-linux-aarch64.rpm",
    "betterdesk-1.5.0-linux-x86_64-portable.tar.gz",
    "betterdesk-1.5.0-linux-x86_64-suse.rpm",
    "betterdesk-1.5.0-linux-x86_64.deb",
    "betterdesk-1.5.0-linux-x86_64.rpm",
    "betterdesk-1.5.0-macos-aarch64.dmg",
    "betterdesk-1.5.0-macos-aarch64.tar.gz",
    "betterdesk-1.5.0-macos-x86_64.dmg",
    "betterdesk-1.5.0-macos-x86_64.tar.gz",
    "betterdesk-1.5.0-windows-aarch64.tar.gz",
    "betterdesk-1.5.0-windows-x86_64.tar.gz",
    "betterdesk-1.5.0-x86_64.exe",
    "betterdesk-1.5.0-x86_64.msi",
    "betterdesk-desktop-manifest.json",
    "betterdesk-template-aarch64.msi",
    "betterdesk-template-x86_64.msi",
    "generator-templates-1.5.0.tar.gz",
]

failures: list[str] = []


def check(condition: bool, message: str) -> None:
    if not condition:
        failures.append(message)


def test_template_msi_is_not_an_installer() -> None:
    for name in ("betterdesk-template-x86_64.msi", "betterdesk-template-aarch64.msi"):
        check(
            gen._classify_desktop(name) is None,
            f"{name} must not be classified as a client installer",
        )


def test_real_msi_is_an_installer() -> None:
    check(
        gen._classify_desktop("betterdesk-1.5.0-x86_64.msi") == ("windows", "x86_64", "msi"),
        "real MSI must be classified as windows/x86_64/msi",
    )


def test_table_links_the_real_msi_not_the_template() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        asset_dir = Path(tmp)
        for name in REAL_RELEASE_ASSETS:
            (asset_dir / name).write_text("x", encoding="utf-8")
        notes = gen.render_notes(
            product="desktop",
            asset_dir=asset_dir,
            repo="UNITRONIX/BetterDesk-Client",
            tag="desktop/1.5.0-dev.23-96dc598",
            release_name="BetterDesk Client 1.5.0 (dev 23)",
        )
    check("betterdesk-template" not in notes, "release notes must not link a template MSI")
    for arch in ("x86_64", "aarch64"):
        expected = f"betterdesk-1.5.0-{arch}.msi"
        check(expected in notes, f"notes must link {expected}")

    # Every download link must point at a file that was actually staged. The tag may itself
    # contain '/', so the file name is taken as the last path segment of the link.
    for match in re.finditer(r"\[[^\]]+\]\((https://github\.com/[^)\s]+)\)", notes):
        url = match.group(1)
        if "/releases/download/" not in url:
            continue
        name = url.rstrip("/").rsplit("/", 1)[-1]
        check(
            name in REAL_RELEASE_ASSETS,
            f"notes link to an asset that was not published: {name}",
        )


def test_duplicate_slot_is_loud() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        asset_dir = Path(tmp)
        # Both claim windows/x86_64/exe; guessing one is how a dead or wrong link ships.
        for name in ("betterdesk-1.5.0-x86_64.exe", "betterdesk-1.5.1-x86_64.exe"):
            (asset_dir / name).write_text("x", encoding="utf-8")
        try:
            gen.index_assets(asset_dir, "desktop")
        except SystemExit:
            return
    failures.append("index_assets must fail on two assets claiming the same cell")


def test_every_arch_row_has_windows_and_linux() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        asset_dir = Path(tmp)
        for name in REAL_RELEASE_ASSETS:
            (asset_dir / name).write_text("x", encoding="utf-8")
        assets = gen.index_assets(asset_dir, "desktop")
    for arch in ("x86_64", "aarch64"):
        for platform, kind in (
            ("windows", "exe"),
            ("windows", "msi"),
            ("linux", "deb"),
            ("linux", "rpm"),
            ("macos", "dmg"),
        ):
            check(
                (platform, arch, kind) in assets,
                f"missing {platform}/{arch}/{kind} in the indexed assets",
            )


def test_notes_keep_markdown_structure_and_utf8() -> None:
    """Release notes must survive the round trip to GitHub as Markdown in UTF-8.

    An earlier repair read the body through a subprocess without `encoding="utf-8"`, which on
    Windows decodes UTF-8 as cp1252 and turned "—" into mojibake; `--jq` also flattened the
    newlines so the table stopped rendering. Rendering is checked here so the generator cannot
    regress into producing a single line or non-ASCII mojibake.
    """
    with tempfile.TemporaryDirectory() as tmp:
        asset_dir = Path(tmp)
        for name in REAL_RELEASE_ASSETS:
            (asset_dir / name).write_text("x", encoding="utf-8")
        notes = gen.render_notes(
            product="desktop",
            asset_dir=asset_dir,
            repo="UNITRONIX/BetterDesk-Client",
            tag="desktop/1.5.0",
            release_name="BetterDesk Client 1.5.0",
        )
    check(notes.count("\n") >= 10, "notes must contain many newlines for Markdown tables")
    check("| Architecture |" in notes, "notes must contain the download table header")
    encoded = notes.encode("utf-8")
    check(encoded.decode("utf-8") == notes, "notes must round-trip as UTF-8")
    for mojibake in ("\u00e2\u20ac", "\u00c3"):
        check(mojibake not in notes, f"notes must not contain mojibake {mojibake!r}")


def test_empty_asset_dir_is_rejected() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        try:
            assets = gen.index_assets(Path(tmp), "desktop")
        except SystemExit:
            return
        check(not assets, "an empty directory must yield no assets, never a phantom link")


def main() -> int:
    test_template_msi_is_not_an_installer()
    test_real_msi_is_an_installer()
    test_table_links_the_real_msi_not_the_template()
    test_duplicate_slot_is_loud()
    test_every_arch_row_has_windows_and_linux()
    test_notes_keep_markdown_structure_and_utf8()
    if failures:
        print("release download table tests failed:", file=sys.stderr)
        for failure in failures:
            print(f"  - {failure}", file=sys.stderr)
        return 1
    print("release download table: OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
