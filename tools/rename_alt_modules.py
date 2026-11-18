#!/usr/bin/env python3
"""Give the new UI's modules plain names now that the old ones are gone.

The new UI was written beside the old one, so its modules are `alt` and `alt_*`. This renames
them: `alt` becomes `frame` (it is the launcher's frame: navigation, the play bar), `alt_home`
becomes `home`, `alt_mods` becomes `mods`, and so on, and the directories beside them
(`alt_mods/`) follow. A module that a file pulls in with `#[path = "alt_x.rs"]` gets a normal
home instead: `src/<file>/x.rs`, declared with a plain `mod x;`.

It stops without changing anything if a new name is already taken. Usage, from the repository
root: tools/rename_alt_modules.py
"""
import os
import re
import subprocess
import sys

MAIN = "src/main.rs"
SPECIAL = {"alt": "frame"}


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def write(path, text):
    with open(path, "w", encoding="utf-8") as f:
        f.write(text)


def git_mv(src, dst):
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    subprocess.run(["git", "mv", src, dst], check=True)


def sources():
    for root, _, files in os.walk("src"):
        for f in files:
            if f.endswith(".rs"):
                yield os.path.join(root, f)
    for root, _, files in os.walk("tests"):
        for f in files:
            if f.endswith(".rs"):
                yield os.path.join(root, f)


def new_name(old):
    return SPECIAL.get(old, old[len("alt_"):])


def main():
    top = re.findall(r"(?m)^mod (alt(?:_\w+)?);", read(MAIN))
    # Modules pulled in with #[path] by another file: (owner file, old name).
    pathed = []
    for f in sources():
        for m in re.finditer(r'#\[path = "(alt_\w+)\.rs"\]\s*mod (\w+);', read(f)):
            pathed.append((f, m.group(1)))
    # Where things go.
    moves = {}  # old path -> new path
    names = {}  # old module name -> new module name
    for old in top:
        new = new_name(old)
        names[old] = new
        if os.path.isfile(f"src/{old}.rs"):
            moves[f"src/{old}.rs"] = f"src/{new}.rs"
        if os.path.isdir(f"src/{old}"):
            moves[f"src/{old}"] = f"src/{new}"
    for owner, old in pathed:
        new = new_name(old)
        names[old] = new
        stem = os.path.splitext(owner)[0]
        moves[f"src/{old}.rs"] = f"{stem}/{new}.rs"
    # Refuse a name that is taken.
    taken = []
    for old, new in moves.items():
        if os.path.exists(new):
            taken.append(new)
    if taken:
        print("already exist:", ", ".join(taken))
        sys.exit(1)
    for old, new in sorted(moves.items()):
        git_mv(old, new)
    # Paths inside #[path] attributes of the moved files, and the declarations themselves.
    for f in list(sources()):
        text = read(f)
        orig = text
        for old, new in names.items():
            # A module another file pulled in by path: now declared plainly.
            text = re.sub(rf'#\[path = "{old}\.rs"\]\s*mod {old};', f"mod {new};", text)
            text = re.sub(rf'#\[path = "{old}/', f'#[path = "{new}/', text)
            text = re.sub(rf"\bmod {old};", f"mod {new};", text)
            text = re.sub(rf"\b(crate|super|self)::{old}\b", rf"\1::{new}", text)
            text = re.sub(rf"(?<![\w.:]){old}::", f"{new}::", text)
            text = re.sub(rf"(?<=::){old}\b(?![\w(])", new, text)
            text = re.sub(rf"\buse {old}\b", f"use {new}", text)
        if text != orig:
            write(f, text)
    for old, new in sorted(moves.items()):
        print(f"{old} -> {new}")


if __name__ == "__main__":
    main()
