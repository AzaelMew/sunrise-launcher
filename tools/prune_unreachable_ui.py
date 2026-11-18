#!/usr/bin/env python3
"""Remove the launcher's old screens once the new UI is the only UI.

The old screens (glass_*, blocks_*, the *_page.rs files, the old frame...) are no longer reached
from the window, so the compiler calls their code unused. This script removes it:

* Item pass: `cargo check --all-targets`, then delete every function, method, constant, static,
  type, struct and enum the compiler reports as never used, and `cargo fix` the imports that
  left unused. Repeat: each removal can make more code unused.
* Module pass: delete the `mod` lines (in src/main.rs) and files of the candidate modules that
  are still there (the old screens), then compile; every error names something a kept module
  still uses, so the module that defines it is put back. Repeat until the tree compiles.

The two passes alternate until neither removes anything. A module that is still needed stays
whole; unused fields, enum variants and the like are left to hand fixes. Code that only a Windows
or macOS build compiles is invisible to a Linux compile, so the names used by the WINDOWS_ONLY
files are never removed.

Usage: tools/prune_unreachable_ui.py   (from the repository root; prints what it removed)
"""
import json
import os
import re
import subprocess
import sys

MAIN = "src/main.rs"

# Candidate old-UI modules, as regular expressions on the module name.
CANDIDATES = [
    r"glass_.*", r"blocks(_.*)?", r"home(_.*)?", r".*_page", r"nav_strip", r"outline_.*",
    r"frame", r"titlebar", r"widgets", r"mods_project", r"instance_mods", r"new_instance",
    r"welcome", r"specimen", r"island", r"launcher_render", r"native_registry_consent",
]
WINDOWS_ONLY = ["src/setup_page.rs"]
KEEP = {"setup_page", "startup", "native_build"}

MOD_RE = re.compile(r"^(\s*)mod (\w+);\s*$")


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


SNAP = None  # while a round of removals runs: file -> its text before the round


def write(path, text):
    if SNAP is not None and path not in SNAP:
        SNAP[path] = read(path) if os.path.exists(path) else None
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def sh(args):
    return subprocess.run(args, capture_output=True, text=True, env=dict(os.environ, CARGO_INCREMENTAL="0"))


def check(targets=("--all-targets",)):
    p = sh(["cargo", "check", *targets, "--message-format=json"])
    errors, warnings = [], []
    for line in p.stdout.splitlines():
        try:
            m = json.loads(line)
        except ValueError:
            continue
        if m.get("reason") != "compiler-message":
            continue
        msg = m["message"]
        (errors if msg["level"] == "error" else warnings).append(msg)
    return errors, warnings


def mods_in_main():
    return {m.group(2): i for i, l in enumerate(read(MAIN).split("\n")) if (m := MOD_RE.match(l))}


def files_of(name):
    out = []
    if os.path.isfile(f"src/{name}.rs"):
        out.append(f"src/{name}.rs")
    if os.path.isdir(f"src/{name}"):
        for root, _, names in os.walk(f"src/{name}"):
            out += [os.path.join(root, n) for n in names]
    return sorted(out)


# ---- item pass ----

def item_end(text, at, is_value):
    """Index just past the item whose name starts at `at`."""
    depth = 0
    i, n = at, len(text)
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            i = text.find("\n", i)
            if i < 0:
                return n
            continue
        if text.startswith("/*", i):
            i = text.find("*/", i) + 2
            continue
        if c == "r" and re.match(r'r#*"', text[i:i + 8]) and not (text[i - 1].isalnum() or text[i - 1] == "_"):
            hashes = len(re.match(r"r(#*)", text[i:]).group(1))
            i = text.find('"' + "#" * hashes, i + 2 + hashes) + 1 + hashes
            continue
        if c == '"':
            i += 1
            while text[i] != '"':
                i += 2 if text[i] == "\\" else 1
            i += 1
            continue
        if c == "'":
            m = re.match(r"'(\\[^']*|[^\\'])'", text[i:])
            if m:
                i += m.end()
                continue
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
            if depth == 0 and c == "}" and not is_value:
                return i + 1
        elif c == ";" and depth == 0:
            return i + 1
        i += 1
    return n


def item_range(text, at):
    """The span of the item whose name is at character `at`: docs and attributes above it, the
    body, and one blank line."""
    line_start = text.rfind("\n", 0, at) + 1
    head = text[line_start:text.find("\n", at)]
    is_value = bool(re.search(r"\b(const|static|type)\b", head)) and not re.search(r"\bfn\b", head)
    end = item_end(text, at, is_value)
    nl = text.find("\n", end)
    end = nl + 1 if nl >= 0 else len(text)
    start = line_start
    while start > 0:
        prev_start = text.rfind("\n", 0, start - 1) + 1
        prev = text[prev_start:start].strip()
        if prev.startswith(("///", "#[", "//")) and not prev.startswith("//!"):
            start = prev_start
        else:
            break
    if text[start - 2:start] == "\n\n" and text[end:end + 1] in ("\n", ""):
        end = min(len(text), end + 1)
    return start, end


def remove_empty_impls(text):
    return re.sub(r"\n[ ]*impl(?:<[^>\n]*>)? [^\n{]*\{\s*\}\n", "\n", text)


def drop_import_name(text, at, end):
    """Remove the name at [at, end) from the `use` statement around it."""
    start = text.rfind("use ", 0, at)
    stop = text.find(";", end) + 1
    stmt = text[start:stop]
    local = at - start
    stmt = stmt[:local] + stmt[local + (end - at):]
    # A glob whose path was dropped (`prelude::*` removed by its span `prelude`).
    stmt = re.sub(r"(?<![\w}])::\*\s*,?", "", stmt)
    # A name that was renamed (`Name as other`) or `self` leaves its tail behind.
    stmt = re.sub(r"^(\s*)as \w+", "", stmt)
    for _ in range(10):
        before = stmt
        stmt = re.sub(r"\{\s*as \w+\s*,?", "{", stmt)
        stmt = re.sub(r",\s*as \w+", "", stmt)
        stmt = re.sub(r",\s*,", ",", stmt)
        stmt = re.sub(r"\{\s*,", "{", stmt)
        stmt = re.sub(r",\s*\}", "}", stmt)
        stmt = re.sub(r"[\w:]*::\{\s*\}", "", stmt)
        stmt = re.sub(r"\{\s*([^{},]+?)\s*\}", r"\1", stmt) if "::{" not in stmt else stmt
        if stmt == before:
            break
    if re.fullmatch(r"use\s*(::)?\s*\*\s*;", stmt.strip()):
        stmt = ""
    if re.fullmatch(r"use\s*(::)?\s*;|use [\w:]*::\s*;|use\s*\{\s*\}\s*;", stmt.strip()) or "::;" in stmt or stmt.strip() in ("use ;", "use"):
        stmt = ""
    return text[:start] + stmt + text[stop:]


def test_fn_around(text, pos):
    """The range of the #[test] function that contains character `pos`, if there is one."""
    for m in re.finditer(r"(?m)^[ ]*#\[test\][^\n]*\n(?:[ ]*#\[[^\n]*\n)*[ ]*(?:async )?fn (\w+)", text):
        a, z = item_range(text, m.start(1))
        if a <= pos < z:
            return a, z
    return None


def trait_impl_around(text, pos):
    """The range of the `impl Trait for Type` block that contains character `pos`, if any:
    when its trait or what it calls was unused, the compiler counted the impl unused too."""
    for m in re.finditer(r"(?m)^[ ]*impl(?:<[^>\n]*>)? [^\n{]* for [^\n{]*\{", text):
        a, z = item_range(text, m.start())
        if a <= pos < z:
            return a, z
    return None


def repair():
    """Fix what removing items left behind: imports of removed names are dropped, and a #[test]
    or trait impl that used a removed item is removed with it. Returns (True, []) when the tree
    compiles, else (False, the errors that cannot be repaired this way)."""
    for _ in range(30):
        errors, _ = check()
        if not errors:
            return True, []
        imports, tests, bad = {}, {}, []
        for e in errors:
            sp = next((x for x in e["spans"] if x["is_primary"] and x["file_name"].startswith("src/")), None)
            if sp is None:
                bad.append(e)
                continue
            if e.get("code") and e["code"]["code"] == "E0432":
                imports.setdefault(sp["file_name"], set()).add((sp["byte_start"], sp["byte_end"]))
            elif e.get("code") and e["code"]["code"] == "E0405":
                # The impl of a trait that was removed goes with it.
                raw = read(sp["file_name"]).encode()
                at = len(raw[:sp["byte_start"]].decode())
                tests.setdefault(sp["file_name"], set()).add(item_range(read(sp["file_name"]), at))
            else:
                raw = read(sp["file_name"]).encode()
                at = len(raw[:sp["byte_start"]].decode())
                around = test_fn_around(read(sp["file_name"]), at) or trait_impl_around(read(sp["file_name"]), at)
                if around is None:
                    bad.append(e)
                    continue
                tests.setdefault(sp["file_name"], set()).add(around)
        if bad:
            return False, bad
        if tests:
            imports = {}  # their offsets are stale once the text changes: next round
        for f, ranges in imports.items():
            raw = read(f).encode()
            text = read(f)
            for b, e in sorted(ranges, reverse=True):
                text = drop_import_name(text, len(raw[:b].decode()), len(raw[:e].decode()))
            write(f, text)
        for f, ranges in tests.items():
            text = read(f)
            for a, z in sorted(ranges, reverse=True):
                text = text[:a] + text[z:]
            write(f, text)
    return False, errors


def fix_unused_imports(frozen):
    """Drop the imports that neither the program nor its tests use: a name the program alone
    leaves unused may still be used by a test, which `cargo fix` would not see."""
    unused = []
    for targets in (("--bin", "sunrise-launcher"), ("--bin", "sunrise-launcher", "--profile", "test")):
        _, warnings = check(targets)
        unused.append({(sp["file_name"], sp["byte_start"], sp["byte_end"])
                       for w in warnings if "unused import" in w["message"]
                       for sp in w["spans"] if sp["is_primary"] and sp["file_name"].startswith("src/")})
    both = unused[0] & unused[1]
    by_file = {}
    for f, b, e in both:
        # An import the Windows-only code uses is unused on Linux but not removable.
        if re.split(r"::|\s+as\s+", read(f).encode()[b:e].decode())[-1].strip() in frozen:
            continue
        by_file.setdefault(f, []).append((b, e))
    for f, ranges in by_file.items():
        raw = read(f).encode()
        text = read(f)
        for b, e in sorted(ranges, reverse=True):
            text = drop_import_name(text, len(raw[:b].decode()), len(raw[:e].decode()))
        write(f, text)
    return len(by_file)


TYPE_WORDS = ("struct", "enum", "type alias")


def culprit_names(errors, removed):
    """Names of the removed items the unrepairable errors say are still needed: the item an error
    names, or the item that held a function or constant of that name."""
    names = set()
    for e in errors:
        for ident in re.findall(r"`([A-Za-z_][A-Za-z0-9_]*)`", e["message"]):
            for name, text in removed:
                if name == ident or re.search(rf"\b(?:fn|const|static)\s+{ident}\b", text):
                    names.add(name)
            names.add(ident)
    return names


def item_pass(frozen):
    """Delete the items the compiler calls unused. Functions, methods and constants go first;
    types only once no function is left. When a round breaks the build in a way imports and
    tests cannot explain, the round is undone, what the errors name is kept, and it runs again.
    Returns the number removed."""
    global SNAP
    total = 0
    types = False
    for _ in range(80):
        # What the program itself never uses; the tests may still use it (see repair).
        errors, warnings = check(("--bin", "sunrise-launcher"))
        if errors:
            print("the tree does not compile before an item round:", errors[0]["rendered"])
            sys.exit(1)
        spans = {}
        for w in warnings:
            m = w["message"]
            if ("never used" not in m and "never constructed" not in m) or "variant" in m:
                continue
            if any(k in m.split("`")[0] for k in TYPE_WORDS) != types:
                continue
            for s in w["spans"]:
                if s["is_primary"] and s["file_name"].startswith("src/") and not s["file_name"].startswith("src/lib"):
                    spans[(s["file_name"], s["byte_start"])] = s
        by_file = {}
        for (f, b), s in spans.items():
            raw = read(f).encode()
            if raw[b:s["byte_end"]].decode() in frozen:
                continue
            # Spans are in bytes, the text is in characters.
            by_file.setdefault(f, []).append(len(raw[:b].decode()))
        if not by_file:
            if types:
                return total
            types = True
            continue
        SNAP = {}
        removed, items = 0, []
        for f, starts in by_file.items():
            text = read(f)
            last = None
            ranges = {}
            for b in starts:
                ranges[item_range(text, b)] = re.match(r"\w+", text[b:]).group(0)
            for (a, z), name in sorted(ranges.items(), reverse=True):
                if last is not None and z > last:  # inside an item already removed
                    continue
                items.append((name, text[a:z]))
                text = text[:a] + text[z:]
                last = a
                removed += 1
            write(f, remove_empty_impls(text))
        ok, bad = repair()
        if not ok:
            keep = culprit_names(bad, items)
            for f, text in SNAP.items():
                if text is None:
                    os.remove(f)
                else:
                    open(f, "w", encoding="utf-8").write(text)
            SNAP = None
            if not keep - frozen:
                print("removing items broke the build; left to hand fixes:", bad[0]["rendered"])
                if types:
                    return total
                types = True
                continue
            frozen |= keep
            continue
        SNAP = None
        total += removed
        fix_unused_imports(frozen)
        types = False
    return total


# ---- module pass ----

class Tree:
    def __init__(self):
        self.removed = {}
        self.order = list(mods_in_main())

    def remove(self, name):
        lines = read(MAIN).split("\n")
        idx = mods_in_main()[name]
        start = idx - 1 if idx > 0 and lines[idx - 1].lstrip().startswith("#[") else idx
        files = {f: read(f) for f in files_of(name)}
        self.removed[name] = {"decl": lines[start:idx + 1], "files": files}
        del lines[start:idx + 1]
        write(MAIN, "\n".join(lines))
        for f in files:
            os.remove(f)
        if os.path.isdir(f"src/{name}"):
            for root, _, _ in sorted(os.walk(f"src/{name}"), reverse=True):
                try:
                    os.rmdir(root)
                except OSError:
                    pass

    def restore(self, name):
        info = self.removed.pop(name)
        lines = read(MAIN).split("\n")
        mods = mods_in_main()
        order = self.order
        later = [n for n in order[order.index(name) + 1:] if n in mods]
        earlier = [n for n in reversed(order[:order.index(name)]) if n in mods]
        if later:
            at = mods[later[0]]
            if at > 0 and lines[at - 1].lstrip().startswith("#["):
                at -= 1
        elif earlier:
            at = mods[earlier[0]] + 1
        else:
            at = len(lines)
        lines[at:at] = info["decl"]
        write(MAIN, "\n".join(lines))
        for f, text in info["files"].items():
            os.makedirs(os.path.dirname(f), exist_ok=True)
            write(f, text)

    def defines(self, ident):
        pat = re.compile(rf"\b(fn|struct|enum|const|static|type|trait|union|macro_rules!|mod)\s+{re.escape(ident)}\b")
        return [n for n, i in self.removed.items() if n == ident or any(pat.search(t) for t in i["files"].values())]


def culprits(tree, errors):
    """Per error, the removed modules defining the most specific name it mentions."""
    need = set()
    for e in errors:
        message, rendered = e["message"], e["rendered"]
        idents = set(re.findall(r"`([A-Za-z_][A-Za-z0-9_]*)`", message))
        idents |= set(re.findall(r"\b(?:crate|super)::([a-z_][a-z0-9_]*)", rendered))
        idents |= set(re.findall(r"\b([a-z_][a-z0-9_]*)::", message))
        found = [m for m in (tree.defines(i) for i in sorted(idents)) if m]
        if found:
            need.update(min(found, key=len))
    return need


def module_pass(keep):
    tree = Tree()
    for n in [n for n in mods_in_main() if n not in keep and any(re.fullmatch(p, n) for p in CANDIDATES)]:
        tree.remove(n)
    while True:
        errors, _ = check()
        if not errors:
            return tree.removed
        need = culprits(tree, errors)
        if not need:
            for e in errors[:20]:
                print("error not explained by a removed module:", e["rendered"])
            sys.exit(1)
        for n in sorted(need):
            tree.restore(n)


def mentions(body):
    """Constants, `path::name` items, calls and method calls in `body`; types are too common
    to follow."""
    found = set(re.findall(r"\b[A-Z][A-Z0-9_]+\b", body))
    found |= set(re.findall(r"::([a-z_][a-z0-9_]*)", body))
    found |= set(re.findall(r"(?<!fn )(?<![:\w])\.?([a-z_][a-z0-9_]*)\(", body))
    return found


def close_over(text):
    """What the Windows-only code mentions, and what the code it reaches mentions in turn: the
    Linux compile cannot see it, so the unused-code passes must leave it."""
    bodies = {}
    for root, _, files in os.walk("src"):
        for f in files:
            if not f.endswith(".rs"):
                continue
            path = os.path.join(root, f)
            src = read(path)
            for m in re.finditer(r"(?m)^([ ]*)(?:pub(?:\([a-z]+\))? )?(?:const |static |(?:async )?fn )(\w+)", src):
                try:
                    a, z = item_range(src, m.start(2))
                except IndexError:  # a name in a comment or string
                    continue
                bodies.setdefault(m.group(2), []).append(src[a:z])
    names = mentions(text)
    todo = list(names)
    while todo:
        for body in bodies.get(todo.pop(), []):
            for ident in mentions(body) - names:
                names.add(ident)
                todo.append(ident)
    return names


def main():
    windows_text = "".join(read(f) for f in WINDOWS_ONLY if os.path.exists(f))
    keep = KEEP | {n for n in mods_in_main() if re.search(rf"\b{n}\b", windows_text)}
    frozen = close_over(windows_text)
    items = 0
    gone = {}
    for round_no in range(1, 6):
        n_items = item_pass(frozen)
        removed = module_pass(keep)
        for name, info in removed.items():
            gone[name] = sum(t.count("\n") + 1 for t in info["files"].values())
        items += n_items
        print(f"round {round_no}: {n_items} items, {len(removed)} modules removed")
        if n_items == 0 and not removed:
            break
    print("modules removed:")
    for n, c in sorted(gone.items()):
        print(f"  {n}: {c} lines")
    print(f"total: {items} items and {len(gone)} modules ({sum(gone.values())} lines in modules)")


if __name__ == "__main__":
    main()
