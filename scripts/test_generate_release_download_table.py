#!/usr/bin/env python3
"""Tests for release download table generation."""

from __future__ import annotations

import re
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from generate_release_download_table import _url, render_table


class ReleaseDownloadTableTests(unittest.TestCase):
    def test_url_escapes_release_tag_as_one_path_segment(self) -> None:
        url = _url(
            "UNITRONIX/BetterDesk-Client",
            "desktop/1.5.0",
            "betterdesk-1.5.0-x86_64.exe",
        )
        self.assertIn("/releases/download/desktop%2F1.5.0/", url)
        self.assertNotIn("/releases/download/desktop/1.5.0/", url)

    def test_desktop_table_links_each_present_asset(self) -> None:
        assets = {
            ("windows", "x86_64", "exe"): "betterdesk-1.5.0-x86_64.exe",
            ("windows", "x86_64", "msi"): "betterdesk-1.5.0-x86_64.msi",
            ("windows", "x86_64", "portable"): "betterdesk-1.5.0-windows-x86_64.tar.gz",
            ("linux", "x86_64", "deb"): "betterdesk-1.5.0-linux-x86_64.deb",
            ("macos", "x86_64", "dmg"): "betterdesk-1.5.0-macos-x86_64.dmg",
        }
        table = render_table(
            product="desktop",
            assets=assets,
            repo="UNITRONIX/BetterDesk-Client",
            tag="desktop/1.5.0",
        )

        for name in assets.values():
            self.assertIn(
                f"/releases/download/desktop%2F1.5.0/{name}",
                table,
            )

    def test_missing_assets_are_not_linked(self) -> None:
        table = render_table(
            product="desktop",
            assets={
                ("windows", "x86_64", "exe"): "betterdesk-1.5.0-x86_64.exe",
            },
            repo="UNITRONIX/BetterDesk-Client",
            tag="desktop/1.5.0",
        )

        self.assertIn("[EXE]", table)
        self.assertNotIn("[MSI]", table)
        self.assertNotRegex(table, re.compile(r"Ubuntu / Linux.*\["))
        self.assertNotRegex(table, re.compile(r"\| Mac \|.*\["))


if __name__ == "__main__":
    unittest.main()
