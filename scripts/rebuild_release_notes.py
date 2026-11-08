#!/usr/bin/env python3
"""Rebuild the notes of released desktop builds from their real assets.

Why this exists: an earlier repair pass read the release body through a subprocess without
`encoding="utf-8"`. On Windows that decodes UTF-8 as cp1252, which turned "—" into mojibake
and, combined with `--jq`, collapsed the Markdown into one line. The table then could not be
fixed by string surgery, so this script regenerates the whole body instead:

  1. list the assets actually attached to the release (the only source of truth for links),
  2. stage empty files with those names (the generator classifies by name),
  3. render the notes with the fixed generator,
  4. re-attach the "Development changes" and "Notes" sections from git and the run id,
  5. verify: no template link, every link resolves to an attached asset.

Every subprocess call passes an explicit encoding, so the body round-trips as UTF-8.

Usage:
    python scripts/rebuild_release_notes.py --tag desktop/1.5.0 --run-id 41080715992
    python scripts/rebuild_release_notes.py --tag ... --run-id ... --apply
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = "UNITRONIX/BetterDesk-Client"
ROOT = Path(__file__).resolve().parents[1]
TAG_PATTERN = re.compile(r"^desktop/(?P<version>[0-9][0-9.]*?)(?:-dev\.[0-9]+-[0-9a-f]+)?$")


def run(cmd: list[str]) -> str:
    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(f"{' '.join(cmd)} failed: {result.stderr.strip()}")
    return result.stdout


def gh(*args: str) -> str:
    return run(["gh", *args, "--repo", REPO])


def release_assets(tag: str) -> list[dict]:
    encoded = tag.replace("/", "%2F")
    raw = run(["gh", "api", f"repos/{REPO}/releases/tags/{encoded}"])
    data = json.loads(raw)
    return data["assets"]


def previous_desktop_tag(commit: str) -> str:
    result = subprocess.run(
        ["git", "describe", "--tags", "--abbrev=0", "--match", "desktop/*", f"{commit}^"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        cwd=ROOT,
        check=False,
    )
    return result.stdout.strip() if result.returncode == 0 else ""


def development_changes(commit: str) -> list[str]:
    previous = previous_desktop_tag(commit)
    if not previous:
        return []
    output = subprocess.run(
        ["git", "log", "--pretty=format:- %s (%h)", f"{previous}..{commit}"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        cwd=ROOT,
        check=False,
    )
    if output.returncode != 0:
        return []
    return [line for line in output.stdout.splitlines() if line.strip()][:80]


def build_extra_md(*, commit: str, run_id: str, channel: str) -> str:
    lines = ["### Development changes", ""]
    changes = development_changes(commit)
    if changes:
        lines.extend(changes)
    else:
        lines.append("- (no commits recorded)")
    lines.append("")
    lines.append("### Notes")
    lines.append("")
    lines.append(
        "- Windows: portable **EXE** and installed **MSI** for x86_64 and ARM64."
    )
    lines.append(
        "- Configure servers in Settings \u2192 Network, or import a deploy string from the BetterDesk panel."
    )
    if run_id:
        lines.append(
            f"- Workflow run: https://github.com/{REPO}/actions/runs/{run_id}"
        )
    if channel == "dev":
        lines.append(
            "- This is a **development prerelease** on the `dev` channel "
            "(update feed: `appcast/dev.json`)."
        )
    else:
        lines.append(
            "- This is a **stable release** on the `main` channel "
            "(update feed: `appcast/stable.json`)."
        )
    lines.append("")
    return "\n".join(lines)


def render_notes(tag: str, version: str, release_name: str, commit: str, channel: str, run_id: str) -> str:
    assets = release_assets(tag)
    names = sorted(asset["name"] for asset in assets)

    with tempfile.TemporaryDirectory() as tmpdir:
        asset_dir = Path(tmpdir)
        for name in names:
            (asset_dir / name).write_bytes(b"")
        extra_md = build_extra_md(commit=commit, run_id=run_id, channel=channel)
        extra_path = asset_dir / "_extra.md"
        extra_path.write_text(extra_md, encoding="utf-8")
        notes_path = asset_dir / "_notes.md"

        subprocess.run(
            [
                sys.executable,
                str(ROOT / "scripts" / "generate_release_download_table.py"),
                "--product",
                "desktop",
                "--asset-dir",
                str(asset_dir),
                "--repo",
                REPO,
                "--tag",
                tag,
                "--release-name",
                release_name,
                "--commit",
                commit,
                "--extra-md-file",
                str(extra_path),
                "-o",
                str(notes_path),
            ],
            check=True,
            cwd=ROOT,
        )
        return notes_path.read_text(encoding="utf-8")


def verify(body: str, names: list[str], tag: str) -> list[str]:
    problems: list[str] = []
    if "betterdesk-template" in body:
        problems.append("links a Generator template as an installer")
    if body.count("\n") < 10:
        problems.append(f"body has only {body.count(chr(10))} newline(s); Markdown will not render")
    for bad in ("â€", "Ă˘", "\u00c3"):
        if bad in body:
            problems.append(f"mojibake detected ({bad!r})")
            break
    for match in re.finditer(r"\[[^\]]+\]\((https://github\.com/[^)\s]+)\)", body):
        url = match.group(1)
        if "/releases/download/" not in url:
            continue
        leaf = url.rstrip("/").rsplit("/", 1)[-1]
        if leaf not in names:
            problems.append(f"link to an asset that is not attached: {leaf}")
    return problems


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--run-id", default="")
    parser.add_argument("--channel", choices=("dev", "stable"), default="")
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()

    match = TAG_PATTERN.match(args.tag)
    if not match:
        raise SystemExit(f"tag does not look like a desktop release: {args.tag}")
    version = match.group("version")

    encoded = args.tag.replace("/", "%2F")
    data = json.loads(run(["gh", "api", f"repos/{REPO}/releases/tags/{encoded}"]))
    release_name = data.get("name") or args.tag
    channel = args.channel or ("dev" if "dev" in args.tag else "stable")
    commit = data.get("target_commitish") or ""

    # The commit is embedded in the existing body as `- Commit: `<sha>``.
    existing = data.get("body") or ""
    commit_match = re.search(r"Commit: `([0-9a-f]{7,40})`", existing)
    if commit_match:
        commit = commit_match.group(1)
    if not commit:
        raise SystemExit("cannot determine the release commit; pass it explicitly")

    body = render_notes(args.tag, version, release_name, commit, channel, args.run_id)
    names = sorted(asset["name"] for asset in release_assets(args.tag))
    problems = verify(body, names, args.tag)
    if problems:
        print(f"{args.tag}: refusing to write", file=sys.stderr)
        for problem in problems:
            print(f"  - {problem}", file=sys.stderr)
        return 1

    print(f"{args.tag}: {len(body)} bytes, {body.count(chr(10))} lines, {len(names)} assets")
    if not args.apply:
        print(body)
        print("\ndry run: re-run with --apply to write")
        return 0

    with tempfile.TemporaryDirectory() as tmpdir:
        path = Path(tmpdir) / "notes.md"
        path.write_text(body, encoding="utf-8", newline="\n")
        gh("release", "edit", args.tag, "--notes-file", str(path))
    print("written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
