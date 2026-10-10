#!/usr/bin/env python3
"""Sign a generic Support Agent manifest with an Ed25519 seed."""

from __future__ import annotations

import argparse
import base64
import os
from pathlib import Path

from nacl.signing import SigningKey


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("manifest", type=Path)
    parser.add_argument("signature", type=Path)
    args = parser.parse_args()
    raw_seed = os.environ.get("BETTERDESK_SUPPORT_RELEASE_SIGNING_SEED", "")
    seed = base64.b64decode(raw_seed, validate=True)
    if len(seed) != 32:
        raise SystemExit("BETTERDESK_SUPPORT_RELEASE_SIGNING_SEED must be base64-encoded 32 bytes")
    signature = SigningKey(seed).sign(args.manifest.read_bytes()).signature
    args.signature.write_text(base64.b64encode(signature).decode("ascii") + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
