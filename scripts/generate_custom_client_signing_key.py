#!/usr/bin/env python3
"""Generate BetterDesk NaCl/Ed25519 keys for signed custom.txt (Client Generator).

Writes:
  res/betterdesk/custom-client-signing.pub
  res/betterdesk/custom-client-signing.seed  (private — keep secret)

Requires: pip install pynacl
"""

from __future__ import annotations

import argparse
import base64
import sys
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=Path(__file__).resolve().parents[1] / "res" / "betterdesk",
    )
    parser.add_argument(
        "--kind",
        choices=("custom-client", "update"),
        default="custom-client",
        help="custom-client: signs custom.txt (panel Generator). "
        "update: signs appcast/<channel>.json (client update feed).",
    )
    args = parser.parse_args()

    try:
        from nacl.signing import SigningKey
    except ImportError:
        print("Install PyNaCl: pip install pynacl", file=sys.stderr)
        return 1

    sk = SigningKey.generate()
    pub_b64 = base64.b64encode(bytes(sk.verify_key)).decode("ascii")
    seed_b64 = base64.b64encode(bytes(sk)).decode("ascii")

    # Two independent keys on purpose: the update key signs what the client installs,
    # the custom-client key signs panel-generated config. Sharing one would let a config
    # signing operation reach the update trust root.
    stem = "custom-client-signing" if args.kind == "custom-client" else "client-update-signing"
    args.out_dir.mkdir(parents=True, exist_ok=True)
    pub_path = args.out_dir / f"{stem}.pub"
    seed_path = args.out_dir / f"{stem}.seed"
    pub_path.write_text(pub_b64 + "\n", encoding="utf-8")
    seed_path.write_text(seed_b64 + "\n", encoding="utf-8")

    print(f"Wrote {pub_path}")
    if args.kind == "custom-client":
        print(f"Wrote {seed_path} (private — for panel Generator only)")
        print("Rebuild the desktop client after updating the public key.")
    else:
        print(f"Wrote {seed_path} (private — CI secret BETTERDESK_CLIENT_RELEASE_SIGNING_SEED)")
        print(
            "Store the seed as the repository secret BETTERDESK_CLIENT_RELEASE_SIGNING_SEED "
            "and rebuild the desktop client so the .pub is embedded."
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
