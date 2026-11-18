#!/usr/bin/env python3
"""The launcher release's tag and notes, for release.yml: `decide` names the release, `notes` writes its title and body."""
import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPOSITORY = "sunrise-org/sunrise-launcher"
# The last version released from the game repository (launcher@1.0.0): installed copies take only a newer one.
SHIPPED_FROM_GAME_REPOSITORY = (1, 0, 0)


def version() -> str:
    """The version in Cargo.toml's [package] section."""
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    package = text.split("[package]", 1)[1].split("\n[", 1)[0]
    found = re.search(r'^version\s*=\s*"([^"]+)"', package, re.M)
    if not found:
        raise SystemExit("Cargo.toml has no [package] version")
    return found.group(1)


def tags() -> list[str]:
    """This repository's release tags (`vX.Y.Z`), newest first."""
    out = subprocess.run(["git", "-C", str(ROOT), "tag", "--list", "v*", "--sort=-v:refname"],
                         capture_output=True, text=True, check=True).stdout
    return [t for t in out.split() if re.fullmatch(r"v\d+\.\d+\.\d+(-[0-9A-Za-z.]+)?", t)]


def decide(head: str) -> dict:
    """The release Cargo.toml's version makes: refused when its tag exists or it isn't newer than what the game repository released."""
    tag = f"v{version()}"
    if tag in tags():
        raise SystemExit(f"{tag} is already released: raise Cargo.toml's version first")
    numbers = tuple(int(n) for n in re.match(r"(\d+)\.(\d+)\.(\d+)", tag[1:]).groups())
    if numbers <= SHIPPED_FROM_GAME_REPOSITORY:
        raise SystemExit(f"{tag} isn't newer than launcher@1.0.0 from the game repository: raise Cargo.toml's version")
    return {"version": tag[1:], "tag": tag, "head": head}


def merged_since(tag: str | None, head: str) -> list[dict]:
    """The pull requests merged into main after `tag` (all, before the first release), from GitHub, newest first."""
    since = ""
    if tag:
        since = subprocess.run(["git", "-C", str(ROOT), "log", "-1", "--format=%cI", tag],
                               capture_output=True, text=True, check=True).stdout.strip()
    search = "is:merged base:main" + (f" merged:>{since}" if since else "")
    out = subprocess.run(["gh", "pr", "list", "-R", REPOSITORY, "--state", "merged", "--search", search,
                          "--limit", "200", "--json", "number,title,labels,mergeCommit"],
                         capture_output=True, text=True, check=True).stdout
    reachable = subprocess.run(["git", "-C", str(ROOT), "rev-list", head], capture_output=True, text=True,
                               check=True).stdout.split()
    return [p for p in json.loads(out) if (p.get("mergeCommit") or {}).get("oid") in reachable]


def notes(tag: str, pulls: list[dict]) -> tuple[str, str]:
    """The release's title and Markdown body: features first, then fixes; `skip-release` pull requests left out."""
    shown = [p for p in pulls if "skip-release" not in {l["name"] for l in p["labels"]}]
    features = [p for p in shown if "feature" in {l["name"] for l in p["labels"]}]
    fixes = [p for p in shown if p not in features]
    lines = []
    for heading, group in (("New", features), ("Fixed and changed", fixes)):
        if group:
            lines += [f"### {heading}", ""] + [f"- {p['title']} (#{p['number']})" for p in group] + [""]
    if not lines:
        lines = ["Maintenance release.", ""]
    lines.append("Each file is listed in SHA256SUMS, signed in SHA256SUMS.sig; the launcher checks both before it updates.")
    return f"Sunrise Launcher {tag[1:]}", "\n".join(lines) + "\n"


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    d = sub.add_parser("decide")
    d.add_argument("--head", required=True)
    n = sub.add_parser("notes")
    n.add_argument("--tag", required=True)
    n.add_argument("--head", required=True)
    n.add_argument("--out", type=Path, required=True)
    args = parser.parse_args(argv)
    if args.command == "decide":
        print(json.dumps(decide(args.head)))
        return 0
    earlier = [t for t in tags() if t != args.tag]
    title, body = notes(args.tag, merged_since(earlier[0] if earlier else None, args.head))
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "title.txt").write_text(title + "\n")
    (args.out / "body.md").write_text(body)
    print(title)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
