#!/usr/bin/env python3
"""Tests for BetterDesk Client version policy."""

from __future__ import annotations

import unittest
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from bump_version import next_patch_version, parse_version


class VersionPolicyTests(unittest.TestCase):
    def test_patch_increments_below_rollover(self) -> None:
        self.assertEqual(next_patch_version("1.5.0"), "1.5.1")
        self.assertEqual(next_patch_version("1.5.99"), "1.5.100")

    def test_patch_100_rolls_to_next_minor(self) -> None:
        self.assertEqual(next_patch_version("1.5.100"), "1.6.0")
        self.assertEqual(next_patch_version("0.0.100"), "0.1.0")

    def test_patch_above_100_is_invalid(self) -> None:
        with self.assertRaises(ValueError):
            parse_version("1.5.101")


if __name__ == "__main__":
    unittest.main()
