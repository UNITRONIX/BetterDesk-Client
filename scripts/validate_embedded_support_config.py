#!/usr/bin/env python3
"""Validate the signed payload accepted by the embedded Support build."""

from __future__ import annotations

import argparse
import base64
import binascii
import json
import os
import sys
from pathlib import Path


def fail(message: str) -> int:
    print(message, file=sys.stderr)
    return 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--payload", default="", help="Signed base64 payload")
    parser.add_argument(
        "--public-key",
        type=Path,
        default=Path(__file__).resolve().parents[1]
        / "res"
        / "betterdesk"
        / "custom-client-signing.pub",
    )
    args = parser.parse_args()
    encoded = args.payload or os.environ.get("BETTERDESK_SUPPORT_CONFIG_B64", "")
    if not encoded:
        return fail("Missing embedded Support payload")

    try:
        from nacl.signing import VerifyKey
    except ImportError:
        return fail("Install PyNaCl: pip install pynacl")

    try:
        public_key = base64.b64decode(args.public_key.read_text(encoding="utf-8").strip(), validate=True)
        signed = base64.b64decode(encoded.strip(), validate=True)
        message = VerifyKey(public_key).verify(signed)
        payload = json.loads(message.decode("utf-8"))
    except (OSError, ValueError, binascii.Error, json.JSONDecodeError) as exc:
        return fail(f"Invalid signed Support payload: {exc}")

    if not isinstance(payload, dict):
        return fail("Support payload must be a JSON object")
    if payload.get("conn-type") != "incoming":
        return fail("Support payload must force conn-type=incoming")
    if payload.get("disable-settings") != "Y":
        return fail("Support payload must disable settings")
    overrides = payload.get("override-settings")
    if not isinstance(overrides, dict):
        return fail("Support payload is missing override-settings")
    required = (
        "custom-rendezvous-server",
        "relay-server",
        "api-server",
        "key",
    )
    if any(not str(overrides.get(key, "")).strip() for key in required):
        return fail("Support payload has incomplete server settings")
    if overrides.get("hide-server-settings") != "Y":
        return fail("Support payload must hide server settings")
    if "default-settings" in payload:
        return fail("Support payload must not expose default-settings")

    print("Embedded Support payload: valid incoming-only profile")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
