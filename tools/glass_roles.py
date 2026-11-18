#!/usr/bin/env python3
"""One-shot migration of the redesign's screens from `tokens` colours to `crate::paint` roles.

Written for the exact R4 base (origin/launcher-r34-r4 @ bafb7cf4a); run it once, before `cargo fmt`:

    python3 tools/glass_roles.py

`ROLES` lists, per file, every base line that names a colour constant and the role it paints:
"NAME>role": [lines]. `NAME` on that line becomes `palette().role`; `CARD>card()` is a plate, the
theme's tint at the plates' opacity. A line that no longer holds its name stops the script. `fn dock`
(frame.rs, 356-688) holds no entry: its constants stay until the dock patch replaces it.
"""
import pathlib
import re
import sys

ROLES = {
    "src/frame.rs": {
        "BG>background": [225, 816], "CARD>chrome_raised": [742], "CARD>raised": [870], "CHROME>chrome":
        [828], "GOLD>accent": [748], "GOLD>focus": [86, 94, 227, 250, 730], "LINE>line": [243, 872],
        "MUTED>chrome_muted": [739, 754], "SOFT>divider": [818], "TEXT>chrome_text": [739, 754],
        "TEXT>text": [223, 224, 246, 829],
    },
    "src/home.rs": {
        "BG>background": [309, 373, 448], "BG>field": [515], "CARD>card()": [201], "GOLD>accent_ink":
        [887, 940], "GOLD>focus": [517], "GOLD>warning": [571, 991], "LINE>line": [517], "MUTED>muted":
        [219, 285, 581, 878], "SOFT>divider": [203, 662, 786, 1032], "TEXT>text": [212, 433, 445, 446,
        481, 519, 679, 796, 870, 874, 1045],
    },
    "src/mods.rs": {
        "CARD>card()": [751, 863], "GOLD>focus": [843], "GOLD>warning": [668], "LINE>line": [654, 749,
        815, 862], "TEXT>text": [659, 667, 747],
    },
    "src/mods/ui.rs": {
        "BG>background": [56, 129, 149], "BG>field": [55, 144], "CARD>card()": [159, 205, 300],
        "GOLD>focus": [59, 89, 116, 143, 210], "LINE>line": [19, 54, 122, 161, 204], "MUTED>muted": [21,
        28, 129, 142, 169, 207, 237, 245, 306], "SOFT>divider": [299], "TEXT>text": [54, 55, 56, 122,
        142, 144, 305],
    },
    "src/mods/discover.rs": {
        "BG>background": [357], "CARD>raised": [139], "LINE>hover": [157], "LINE>line": [138],
        "MUTED>muted": [181], "TEXT>text": [30],
    },
    "src/mods/installed_view.rs": {
        "BG>background": [148, 404], "BLUE>info": [592], "CARD>raised": [861], "DANGER>danger": [135,
        291], "GOLD>focus": [747], "GOLD>warning": [259, 275, 310, 348, 353, 546, 549, 553, 562, 576],
        "LINE>line": [98, 739, 860], "MUTED>muted": [408, 634, 672, 745, 792, 797, 814], "SOFT>divider":
        [166, 463, 658], "TEXT>text": [27, 146, 147, 739, 745],
    },
    "src/mods/project.rs": {
        "CARD>card()": [23], "CARD>card_color": [52], "GOLD>accent_ink": [96], "LINE>line": [196, 207],
        "MUTED>muted": [67, 81, 95, 198, 209, 241, 250, 319, 336, 370, 386], "SOFT>divider": [53, 230],
        "TEXT>text": [117],
    },
    "src/mods/packs.rs": {
        "LINE>selected": [268], "MUTED>muted": [52, 276], "SOFT>divider": [262], "TEXT>text": [45],
    },
    "src/mods/pack_discover.rs": {
        "BG>background": [517], "CARD>raised": [328], "GOLD>focus": [304], "LINE>line": [296, 327],
        "MUTED>muted": [302], "TEXT>text": [296, 302, 385],
    },
    "src/mods/pack_project.rs": {
        "MUTED>muted": [249, 320, 328], "SOFT>divider": [306], "TEXT>text": [112],
    },
    "src/mods/data_packs.rs": {
        "LINE>line": [566], "SOFT>divider": [470, 580], "TEXT>text": [654],
    },
    "src/build.rs": {
        "CARD>card()": [138], "LINE>line": [36], "MUTED>muted": [113, 140], "SOFT>divider": [137],
        "TEXT>text": [38],
    },
    "src/internet/hosting_share.rs": {
        "BG>field": [65, 467], "CARD>card()": [79], "GOLD>focus": [64], "LINE>line": [64],
        "MUTED>muted": [84, 472], "SOFT>divider": [78, 187, 338, 469], "TEXT>text": [67],
    },
    "src/servers_page/hosting.rs": {
        "BAD>danger": [115, 265, 1655], "BG>background": [45], "BG>field": [121], "BG>selected": [1542],
        "CARD>card()": [40, 755], "GOLD>accent_ink": [721, 788, 1668], "GOLD>focus": [117, 1534, 1667],
        "GOLD>warning": [91, 94, 507, 521], "LINE>line": [93, 119, 1612, 1798], "MUTED>muted": [29, 476,
        735, 766, 1548, 1616], "OK>success": [92, 511], "SOFT>divider": [39, 787, 1759], "TEXT>text":
        [45, 123, 236, 758, 1548, 1549, 1790],
    },
    "src/settings.rs": {
        "BAD>danger": [159, 165], "BG>field": [1039, 1266], "CARD>card()": [79], "CARD>raised": [883],
        "CARD>selected": [189], "GOLD>focus": [852, 1268], "LINE>line": [844, 882, 1268], "MUTED>muted":
        [92, 188, 850, 1044], "SOFT>divider": [78, 84, 1041], "TEXT>text": [188, 190, 235, 844, 850,
        1271],
    },
    "src/sheets.rs": {
        "BAD>danger": [858, 1029, 1262], "BG>background": [155, 567, 588, 640], "BG>field": [700],
        "BG>selected": [1126], "CARD>raised": [233, 1305], "GOLD>accent": [179, 1134, 1135],
        "LINE>line": [154, 232, 566, 601, 1244, 1258, 1304], "MUTED>muted": [161, 178, 676, 685, 703,
        1134, 1253, 1334], "SOFT>divider": [993, 1329], "TEXT>text": [184, 234, 586, 587, 640, 1306],
    },
    "src/sheets/versions.rs": {
        "BAD>danger": [436], "BG>background": [367], "BG>field": [223], "BG>hover": [331],
        "BG>selected": [293], "GOLD>focus": [225], "LINE>line": [225, 429], "MUTED>muted": [269, 300],
        "TEXT>text": [300, 301, 365, 366],
    },
    "src/owner_page.rs": {
        "BAD>danger": [197], "BG>background": [53], "CARD>card()": [70], "GOLD>focus": [209],
        "LINE>line": [69], "MUTED>muted": [78, 132, 153, 190, 194], "SOFT>divider": [52], "TEXT>text":
        [71, 91, 96],
    },
}

# Literal paints, everywhere they occur in those files (never in `fn dock`).
LITERALS = [
    (r"\bc\(0x7fd6a0\)", "c(palette().success)"),
    (r"\brgb\(0x7fd6a0\)", "rgb(palette().success)"),
    (r"gpui::black\(\)\.opacity\(0\.55\)", "palette().scrim()"),
    # A text-filled primary button's hover.
    (r"(\.hover\(\|\w+\| \w+\.bg\()(?:rgb\(0xffffff\)|gpui::white\(\))\)", r"\1rgb(palette().accent_on))"),
]
# Exact text that stands once: (file, old, new).
ONCE = [
    ("src/frame.rs", "rgb(0x393233)", "rgb(palette().hover)"),
    ("src/home.rs", "band.bg(rgb(0x2b2627))", "band.bg(rgb(palette().selected))"),
    ("src/mods/data_packs.rs", "c(CARD).opacity(0.6)", "c(palette().hover)"),
    ("src/mods/installed_view.rs", ".bg(c(0x2a2724))", ".bg(c(palette().selected))"),
    ("src/mods/installed_view.rs", "            0x9a948a,", "            palette().muted,"),
    ("src/mods/project.rs", "c(0xcfc9c0)", "c(palette().text)"),
    # Nothing paints with it now; theme.rs's own test still reads it.
    ("src/theme.rs", "pub const SELECTION: f32 = 0.22;", "#[cfg(test)]\npub const SELECTION: f32 = 0.22;"),
    # The frame starts each frame's palette.
    ("src/frame.rs", "        scrolls_begin();\n", "        crate::paint::begin(&self.settings, &self.theme);\n        scrolls_begin();\n"),
    # The tooltip, and a field's selection and placeholders.
    ("src/glass_ui.rs", ".bg(rgb(look().base))", ".bg(rgb(palette().raised))"),
    ("src/glass_ui.rs", ".border_color(white(0.12))\n            .font_family", ".border_color(rgb(palette().line))\n            .font_family"),
    ("src/glass_ui.rs", ".text_color(white(ink::FULL))\n            .child(self.0.clone())", ".text_color(rgb(palette().text))\n            .child(self.0.clone())"),
    ("src/line_edit.rs", ".bg(t.accent.opacity(theme::SELECTION)),", ".bg(rgb(palette().selected)),"),
    ("src/line_edit.rs", "        let color = if t.glass {\n            crate::glass_ui::white(sunrise_launcher_theme::glass::ink::PLACEHOLDER)\n        } else {\n            t.muted\n        };\n        return div().text_color(color)",
     "        return div().text_color(rgb(palette().muted))"),
    ("src/line_edit.rs", "let color = crate::glass_ui::white(sunrise_launcher_theme::glass::ink::PLACEHOLDER);", "let color = rgb(palette().muted);"),
    ("src/line_edit.rs", "use gpui::{\n    black,", "use gpui::{\n    black, rgb,"),
]
MOVED = ["BAD", "BG", "CARD", "SOFT"]  # nothing but `fn dock` names the other five any more


def read(path):
    return pathlib.Path(path).read_text()


def move_names(path, text):
    lines = text.split("\n")
    for key, numbers in ROLES[path].items():
        name, role = key.split(">")
        pattern = r"(?<![\w:.])(?:(?:crate::)?(?:frame|tokens)::)?%s\b(?![\w(!])" % name
        replacement = f"palette().{role}"
        if role == "card()":
            pattern = r"\.bg\((?:rgb|c)\((?:(?:crate::)?frame::)?CARD\)\)"
            replacement = ".bg(palette().card())"
        for n in numbers:
            moved = re.sub(pattern, replacement, lines[n - 1])
            if moved == lines[n - 1]:
                sys.exit(f"{path}:{n}: expected {name}")
            lines[n - 1] = moved
    return "\n".join(lines)


def main():
    if not pathlib.Path("src/paint.rs").exists():
        sys.exit("run from the checkout root with palette.patch applied")
    files = {p: read(p) for p in {*ROLES, *(o[0] for o in ONCE)}}
    for path in ROLES:
        text = move_names(path, files[path])
        for old, new in LITERALS:
            text = re.sub(old, new, text)
        # Host's and Mods' own colour constants are roles now.
        text = re.sub(r"^(?:[ \t]*///[^\n]*\n)*const (?:OK|BLUE|DANGER): u32 = [^;]*;\n", "", text, flags=re.M)
        # The migrated names leave `use crate::frame::{..}`; `palette` comes in.
        def keep(m):
            names = [n for n in re.split(r"\s*,\s*", m.group(2).strip().rstrip(",")) if n and n not in ROLES_NAMES]
            body = names[0] if len(names) == 1 else "{" + ", ".join(names) + "}"
            return f"{m.group(1)}use crate::frame::{body};\n" if names else ""
        text = re.sub(r"^([ \t]*)use crate::frame::\{([^}]*)\};\n", keep, text, flags=re.M)
        files[path] = text
    for path, old, new in ONCE:
        if files[path].count(old) != 1:
            sys.exit(f"{path}: expected one of {old!r}")
        files[path] = files[path].replace(old, new)
    for path in files:
        if "palette()" in files[path] and path != "src/theme.rs":
            end = list(re.finditer(r"^use [^;]*;\n", files[path], re.M))[-1].end()
            files[path] = files[path][:end] + "use crate::paint::palette;\n" + files[path][end:]
    # The four constants nothing names, and the frame's re-export of the rest.
    tokens = files["src/tokens.rs"] = read("src/tokens.rs")
    for name in MOVED:
        tokens = re.sub(r"^(?:///[^\n]*\n)*pub\(crate\) const %s: u32 = [^;]*;\n" % name, "", tokens, flags=re.M)
    files["src/tokens.rs"] = tokens
    frame = files["src/frame.rs"]
    files["src/frame.rs"] = frame.replace("{BAD, BG, CARD, CHROME, GOLD, LINE, MUTED, SOFT, TEXT}", "{CHROME, GOLD, LINE, MUTED, TEXT}")
    for path, text in files.items():
        pathlib.Path(path).write_text(text)
    print(f"{len(files)} files rewritten; run cargo fmt")


ROLES_NAMES = {key.split(">")[0] for roles in ROLES.values() for key in roles}
if __name__ == "__main__":
    main()
