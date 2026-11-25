#!/usr/bin/env python3
"""Fail when a literal `tr`/`trf` key added to the launcher is absent from the catalog.

The English source text is the lookup key (`docs/architecture.md`, "Translations"), so a typo or
an unseeded key silently falls back to English. This check reads the literals passed to `tr`
and `trf` in `src/**/*.rs` and compares them with a runtime catalog.

By default only keys on lines added since `--base` are checked, so the existing drift (keys
seeded before this check existed) does not fail an unrelated pull request. `--all` checks every
literal in the tree, for when the catalogs are known to be complete.
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DEFAULT_CATALOG = ROOT / "lang" / "fr.json"
# `tr(` and `trf(` as whole words; the literal may follow on another line or behind `r"`.
CALL = re.compile(r"\btr(?:f)?\s*\(")


def decode_rust_string(text: str, start: int) -> tuple[str | None, int]:
    """The literal starting at `start` (`"` or `r#"`), decoded, and the index after it.

    None when the first argument is not a plain string literal (an expression, a constant).
    """
    i = start
    while i < len(text) and text[i] in " \t\r\n":
        i += 1
    if i >= len(text):
        return None, i

    def unescape(body: str) -> str:
        out = []
        j = 0
        while j < len(body):
            ch = body[j]
            if ch != "\\" or j + 1 >= len(body):
                out.append(ch)
                j += 1
                continue
            nxt = body[j + 1]
            simple = {"n": "\n", "r": "\r", "t": "\t", "0": "\0", "\\": "\\", '"': '"', "'": "'"}
            if nxt in simple:
                out.append(simple[nxt])
                j += 2
            elif nxt == "u" and j + 2 < len(body) and body[j + 2] == "{":
                end = body.find("}", j + 3)
                if end == -1:
                    out.append("\\u")
                    j += 2
                else:
                    try:
                        out.append(chr(int(body[j + 3 : end], 16)))
                    except ValueError:
                        out.append(body[j : end + 1])
                    j = end + 1
            elif nxt == "\n":
                j += 2  # line continuation
            else:
                out.append(nxt)
                j += 2
        return "".join(out)

    if text.startswith('r"', i) or text.startswith('r#"', i):
        hashes = 0
        j = i + 1
        while j < len(text) and text[j] == "#":
            hashes += 1
            j += 1
        if j >= len(text) or text[j] != '"':
            return None, i
        close = '"' + "#" * hashes
        end = text.find(close, j + 1)
        if end == -1:
            return None, len(text)
        return text[j + 1 : end], end + len(close)
    if text[i] != '"':
        return None, i
    j = i + 1
    while j < len(text):
        if text[j] == "\\":
            j += 2
            continue
        if text[j] == '"':
            return unescape(text[i + 1 : j]), j + 1
        j += 1
    return None, len(text)


def keys_in(text: str):
    """Every literal first argument to `tr`/`trf` in `text`, with its 1-based line."""
    for match in CALL.finditer(text):
        start = match.end()
        while start < len(text) and text[start] in " \t\r\n":
            start += 1
        key, _ = decode_rust_string(text, start)
        if key is not None:
            yield key, text.count("\n", 0, start) + 1


def missing_keys(keys, catalog: dict[str, str]):
    return [(key, line) for key, line in keys if key not in catalog]


def added_lines(base: str, head: str, paths: list[str]) -> dict[str, list[int]]:
    """The 1-based line numbers added in `head` since `base`, per file (0 context)."""
    diff = subprocess.run(
        ["git", "diff", "--no-color", "--unified=0", base, head, "--", *paths],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    added: dict[str, list[int]] = {}
    current = None
    for line in diff.splitlines():
        if line.startswith("+++ b/"):
            current = line[6:]
            added.setdefault(current, [])
        elif line.startswith("@@") and current is not None:
            match = re.search(r"\+(\d+)(?:,(\d+))?", line)
            if not match:
                continue
            start, count = int(match.group(1)), int(match.group(2) or 1)
            added[current].extend(range(start, start + count))
    return added


def check(base: str | None, head: str, catalog_path: Path, everything: bool) -> int:
    catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    paths = sorted(str(p.relative_to(ROOT)) for p in (ROOT / "src").rglob("*.rs"))
    failures = 0
    if everything or not base:
        for path in paths:
            text = (ROOT / path).read_text(encoding="utf-8")
            for key, line in missing_keys(keys_in(text), catalog):
                print(f"{path}:{line}: tr key not in {catalog_path.name}: {key!r}")
                failures += 1
        return failures

    changed = added_lines(base, head, ["src"])
    for path, lines in sorted(changed.items()):
        if not lines or not path.endswith(".rs"):
            continue
        text = (ROOT / path).read_text(encoding="utf-8")
        wanted = set(lines)
        for key, line in keys_in(text):
            if line in wanted and key not in catalog:
                print(f"{path}:{line}: tr key not in {catalog_path.name}: {key!r}")
                failures += 1
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", help="git ref the pull request starts from")
    parser.add_argument("--head", default="HEAD", help="git ref to check (default HEAD)")
    parser.add_argument("--catalog", type=Path, default=DEFAULT_CATALOG)
    parser.add_argument("--all", action="store_true", help="check every literal, not only added lines")
    args = parser.parse_args()
    failures = check(args.base, args.head, args.catalog, args.all)
    if failures:
        print(
            f"\n{failures} literal tr/trf key(s) missing from {args.catalog.name}; "
            "seed them with tools/launcher_add_lang_keys.py",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
