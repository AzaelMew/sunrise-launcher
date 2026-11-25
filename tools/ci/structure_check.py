"""The refactor's ratchet (docs/refactoring.md §8): counts that may only go down between a base and a head revision.

Both revisions are read as git objects, so neither needs a checkout. There is no stored baseline: the base commit is
the baseline, and a pull request that lowers a count needs nothing extra.
"""
import argparse
import re
import subprocess
import sys

RETIRED_PREFIXES = ("glass_", "blocks_", "alt_", "launcher_")
LONG_FILE = 1000

PATH_ATTRIBUTE = re.compile(r"#\[\s*path\s*=")
ALLOW = re.compile(r"#!?\[\s*(?:cfg_attr\s*\(.*?,\s*)?allow\s*\(([^()]*)\)")
CFG_TEST = re.compile(r"#\[\s*cfg\s*\(\s*test\s*\)\s*\]")
FN = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)")

LABELS = {
    "src_files": "files directly under src/",
    "long_files": f".rs files over {LONG_FILE} lines",
    "path_attributes": "#[path attributes",
    "allow_dead_code": "#[allow(dead_code)]",
    "allow_clippy": "#[allow(clippy::…)]",
    "unwraps": ".unwrap() in non-test code",
    "string_errors": "Result<_, String> in non-test code",
    "retired_names": "paths with retired prefixes",
    "stale_manual_tests": "manual-tests.txt entries naming no fn",
}


def git(*args: str, data: bytes | None = None) -> bytes:
    return subprocess.run(["git", *args], input=data, check=True, capture_output=True).stdout


def tree(rev: str) -> list[tuple[str, str]]:
    """(blob id, path) for every file at `rev`."""
    entries = []
    for line in git("ls-tree", "-r", "-z", "--full-tree", rev).split(b"\0"):
        if not line:
            continue
        meta, path = line.split(b"\t", 1)
        _mode, kind, blob = meta.split()
        if kind == b"blob":
            entries.append((blob.decode(), path.decode("utf-8", "surrogateescape")))
    return entries


def blobs(ids: list[str]) -> list[str]:
    """The contents of many blobs in one `git cat-file --batch` call."""
    if not ids:
        return []
    out = git("cat-file", "--batch", data="".join(f"{blob}\n" for blob in ids).encode())
    texts, at = [], 0
    for _ in ids:
        header_end = out.index(b"\n", at)
        size = int(out[at:header_end].split()[2])
        start = header_end + 1
        texts.append(out[start:start + size].decode("utf-8", "replace"))
        at = start + size + 1
    return texts


def is_test_file(path: str) -> bool:
    parts = path.split("/")
    name = parts[-1]
    return (
        "tests" in parts[:-1]
        or name == "tests.rs"
        or name.endswith("_tests.rs")
        or any(part in ("test_support", "test_support.rs") for part in parts)
    )


def non_test_code(path: str, text: str) -> str:
    """The part of a file that counts as non-test code: none of a test file, and nothing from the first #[cfg(test)]."""
    if is_test_file(path):
        return ""
    found = CFG_TEST.search(text)
    return text[:found.start()] if found else text


def string_errors(text: str) -> int:
    """`Result<T, String>` with any T, nested generics included."""
    count = 0
    for found in re.finditer(r"\bResult\s*<", text):
        depth, comma, at = 1, None, found.end()
        while at < len(text) and depth:
            char = text[at]
            if char in "<([":
                depth += 1
            elif char in ">)]" and text[at - 1] != "-":
                depth -= 1
            elif char == "," and depth == 1 and comma is None:
                comma = at
            elif char in ";{}":
                break
            at += 1
        if depth == 0 and comma is not None and text[comma + 1:at - 1].strip() == "String":
            count += 1
    return count


def has_retired_name(path: str) -> bool:
    return any(part.startswith(RETIRED_PREFIXES) for part in path.split("/"))


def manual_test_names(text: str) -> list[str]:
    """The last path segment of each test that tools/ci/manual-tests.txt names."""
    names = []
    for line in text.splitlines():
        fields = line.split("#", 1)[0].split()
        if len(fields) >= 2:
            names.append(fields[1].rsplit("::", 1)[-1])
    return names


def measure(rev: str) -> dict[str, int]:
    files = tree(rev)
    rust = [(blob, path) for blob, path in files if path.endswith(".rs")]
    counts = dict.fromkeys(LABELS, 0)
    counts["src_files"] = sum(1 for _, path in files if path.startswith("src/") and path.count("/") == 1)
    counts["retired_names"] = sum(1 for _, path in files if has_retired_name(path))
    functions = set()
    for (_, path), text in zip(rust, blobs([blob for blob, _ in rust])):
        counts["long_files"] += len(text.splitlines()) > LONG_FILE
        counts["path_attributes"] += len(PATH_ATTRIBUTE.findall(text))
        for lints in ALLOW.findall(text):
            for lint in (part.strip() for part in lints.split(",")):
                counts["allow_dead_code"] += lint == "dead_code"
                counts["allow_clippy"] += lint.startswith("clippy::")
        code = non_test_code(path, text)
        counts["unwraps"] += code.count(".unwrap()")
        counts["string_errors"] += string_errors(code)
        functions.update(FN.findall(text))
    manual = [blob for blob, path in files if path == "tools/ci/manual-tests.txt"]
    if manual:
        names = manual_test_names(blobs(manual)[0])
        counts["stale_manual_tests"] = sum(1 for name in names if name not in functions)
    return counts


def table(base: dict[str, int], head: dict[str, int]) -> str:
    width = max(len(label) for label in LABELS.values())
    lines = [f"{'':{width}}  {'base':>6}  {'head':>6}  {'change':>6}"]
    for key, label in LABELS.items():
        change = head[key] - base[key]
        mark = "  went up" if change > 0 else ""
        lines.append(f"{label:{width}}  {base[key]:>6}  {head[key]:>6}  {change:>+6}{mark}")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base", required=True, help="the revision the change is based on")
    parser.add_argument("--head", default="HEAD", help="the revision to check (default: HEAD)")
    args = parser.parse_args(argv)
    base, head = measure(args.base), measure(args.head)
    print(table(base, head))
    risen = [LABELS[key] for key in LABELS if head[key] > base[key]]
    if risen:
        print(f"\nThese went up, and may only go down (docs/refactoring.md §8): {', '.join(risen)}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
