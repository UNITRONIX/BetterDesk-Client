"""End-to-end check of the repaired releases: encoding, structure, and every link."""

import json
import subprocess
import sys

REPO = "UNITRONIX/BetterDesk-Client"
TAGS = ["desktop/1.5.0", "desktop/1.5.0-dev.23-96dc598"]


def api(tag: str) -> dict:
    encoded = tag.replace("/", "%2F")
    raw = subprocess.run(
        ["gh", "api", f"repos/{REPO}/releases/tags/{encoded}"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=True,
    ).stdout
    return json.loads(raw)


def http_status(url: str) -> str:
    result = subprocess.run(
        ["curl.exe", "-s", "-o", "NUL", "-w", "%{http_code}", "-L", "--max-time", "30", url],
        capture_output=True,
        text=True,
        check=False,
    )
    return result.stdout.strip()


failures = []
for tag in TAGS:
    data = api(tag)
    body = data.get("body") or ""
    names = {asset["name"] for asset in data["assets"]}
    print(f"== {tag}")
    print(f"   {len(body)} chars, {body.count(chr(10))} newlines, {len(names)} assets")

    if body.count("\n") < 10:
        failures.append(f"{tag}: Markdown collapsed to one line")
    for bad in ("â€", "Ă˘", "\u00c3"):
        if bad in body:
            failures.append(f"{tag}: mojibake {bad!r}")
            break
    if "betterdesk-template" in body:
        failures.append(f"{tag}: links a Generator template as an installer")

    import re
    links = re.findall(r"\[[^\]]+\]\((https://github\.com/[^)\s]+)\)", body)
    download = [u for u in links if "/releases/download/" in u]
    missing = [u for u in download if u.rstrip("/").rsplit("/", 1)[-1] not in names]
    if missing:
        failures.append(f"{tag}: {len(missing)} link(s) to unattached assets")

    bad_codes = []
    for url in download:
        code = http_status(url)
        if code != "200":
            bad_codes.append((code, url.rsplit("/", 1)[-1]))
    print(f"   download links: {len(download)}, non-200: {len(bad_codes)}")
    for code, name in bad_codes:
        failures.append(f"{tag}: HTTP {code} for {name}")

print()
if failures:
    print("FAILURES:")
    for failure in failures:
        print(f"  - {failure}")
    sys.exit(1)
print("all repaired releases verified: correct encoding, real installers, every link 200")
