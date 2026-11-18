#!/usr/bin/env python3
"""Replace the new UI's literal type sizes, corner radii and control heights with the tokens in
src/tokens.rs.

A size that is on the scale becomes its token. One that is half a pixel or a pixel or two off
the scale becomes the nearest token, which is the point of having a scale; MAP below lists every
such move. Gaps, padding and a single screen's own layout sizes are left alone (src/tokens.rs
says why).

Usage, from the repository root: tools/sizes_to_tokens.py
Then: rustfmt --edition 2021 on the files it names, and cargo check.
"""
import re
import sys

FILES = [
    "src/build.rs",
    "src/frame.rs",
    "src/home.rs",
    "src/mods.rs",
    "src/mods/data_packs.rs",
    "src/mods/discover.rs",
    "src/mods/installed_view.rs",
    "src/mods/pack_discover.rs",
    "src/mods/pack_project.rs",
    "src/mods/packs.rs",
    "src/mods/project.rs",
    "src/mods/ui.rs",
    "src/servers_page/hosting.rs",
    "src/settings.rs",
    "src/sheets.rs",
    "src/sheets/versions.rs",
]

# method -> {literal: token}
MAP = {
    "text_size": {
        "10.5": "CAPTION", "11.": "CAPTION", "11.5": "CAPTION",
        "12.": "SMALL", "12.5": "SMALL",
        "13.": "LABEL", "13.5": "LABEL",
        "14.": "BODY", "15.": "BODY",
        "16.": "LEAD",
        "17.": "HEADING", "18.": "HEADING",
        "22.": "TITLE", "24.": "TITLE",
        "26.": "DISPLAY", "28.": "DISPLAY", "30.": "DISPLAY",
    },
    "rounded": {
        "2.": "R_BAR",
        "4.": "R_SMALL", "5.": "R_SMALL", "6.": "R_SMALL",
        "8.": "R_CONTROL", "9.": "R_CONTROL", "10.": "R_CONTROL", "11.": "R_CONTROL",
        "12.": "R_CARD", "14.": "R_CARD",
        "16.": "R_PANEL",
    },
    "rounded_l": {"18.": "R_PANEL"},
    "h": {"32.": "CONTROL", "36.": "FIELD", "38.": "FIELD", "44.": "ACTION"},
}

CALL = re.compile(r"\.(text_size|rounded|rounded_l|h)\(px\(([0-9]+\.[0-9]*)\)\)")


def main():
    left = []
    for path in FILES:
        with open(path, encoding="utf-8") as f:
            text = f.read()

        def swap(m):
            token = MAP[m.group(1)].get(m.group(2))
            if token is None:
                if m.group(1) != "h":
                    left.append(f"{path}: {m.group(0)}")
                return m.group(0)
            return f".{m.group(1)}(px(tokens::{token}))"

        new = CALL.sub(swap, text)
        if new != text:
            with open(path, "w", encoding="utf-8") as f:
                f.write(new)
            print(path)
    if left:
        sys.exit("off the scale, and not in MAP:\n" + "\n".join(left))


if __name__ == "__main__":
    main()
