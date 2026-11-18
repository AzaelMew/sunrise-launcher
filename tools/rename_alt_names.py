#!/usr/bin/env python3
"""Drop the `alt` prefix from the new UI's names, now that the old UI is gone.

* Functions and methods `alt_x` and `y_alt_x` become `x` and `y_x`, unless a function `x` already exists; those listed
  in OVERRIDES get the name given there instead. The script stops and lists any collision that
  OVERRIDES does not settle.
* The types `AltHomeState`, `AltModsState` and `AltSheetsState` lose their `Alt`.
* The `Launcher` field `alt` becomes `ui`.

Element ids such as "alt-play" are strings the window never shows, and are left alone. The
compiler checks the result; run `cargo check --all-targets` after it.

Usage, from the repository root: tools/rename_alt_names.py
"""
import os
import re
import sys

# A collision with an existing function, settled by hand: old name -> new name.
OVERRIDES = {
    "alt_go": "play_to",
    "alt_mods_install": "install_registry_mod",
    "alt_pack_done": "pack_installed",
    "alt_play": "play_game",
}

KEYWORDS = {
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "gen", "try",
}
FIELD = "ui"


def files():
    for top in ("src", "tests"):
        for root, _, names in os.walk(top):
            for n in names:
                if n.endswith(".rs"):
                    yield os.path.join(root, n)


def read(p):
    with open(p, encoding="utf-8") as f:
        return f.read()


def main():
    texts = {f: read(f) for f in files()}
    everything = "\n".join(texts.values())
    olds = sorted(set(re.findall(r"\bfn (alt_\w+|\w+_alt_\w+)", everything)))
    # The methods the Launcher already has: a renamed one must not take their names.
    methods = set()
    for text in texts.values():
        impl = None
        for line in text.split("\n"):
            m = re.match(r"impl\b.*?\b(\w+)\s*(?:<[^>]*>)?\s*\{", line)
            if m:
                impl = m.group(1)
            elif line.startswith("}"):
                impl = None
            m = re.match(r"\s+(?:pub(?:\([a-z]+\))? )?fn (\w+)", line)
            if m and impl == "Launcher":
                methods.add(m.group(1))
    renames, collisions = {}, []
    for old in olds:
        new = OVERRIDES.get(old, old[len("alt_"):] if old.startswith("alt_") else old.replace("_alt_", "_"))
        if old not in OVERRIDES and (new in KEYWORDS or new in methods):
            collisions.append((old, new))
            continue
        renames[old] = new
    if collisions:
        for old, new in collisions:
            print(f"collision: {old} -> {new} already exists; add it to OVERRIDES")
        sys.exit(1)
    # Every other `alt_x` word (a call, a path) is one of these functions; an unknown one is a bug.
    stray = sorted(set(re.findall(r"\b(?:alt_\w+|\w+_alt_\w+)", everything)) - set(renames))
    if stray:
        print("alt_ names without a definition:", ", ".join(stray))
        sys.exit(1)
    types = {t: t[len("Alt"):] for t in set(re.findall(r"\bAlt[A-Z]\w*State\b", everything))}
    for f, text in texts.items():
        orig = text
        for old, new in renames.items():
            text = re.sub(rf"\b{old}\b", new, text)
        for old, new in types.items():
            text = re.sub(rf"\b{old}\b", new, text)
        text = re.sub(r"\.alt\b(?!_)", f".{FIELD}", text)
        text = re.sub(r"(?m)^(\s*(?:pub(?:\([a-z]+\))? )?)alt:", rf"\1{FIELD}:", text)
        if text != orig:
            with open(f, "w", encoding="utf-8") as g:
                g.write(text)
    for old, new in sorted(renames.items()):
        print(f"{old} -> {new}")


if __name__ == "__main__":
    main()
