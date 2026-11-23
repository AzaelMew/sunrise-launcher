#!/usr/bin/env python3
"""Rewrite managed Play's `unsettled()` refusals into `unsettled!()`, which names the check that
refused (and the error it discarded, when one was in hand)."""
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FILES = [ROOT / "src/managed_play.rs", *sorted((ROOT / "src/managed_play").rglob("*.rs"))]

RULES = [
    # An error the check had and dropped: keep it as the reason.
    (re.compile(r"\|(_e|_error|_)\| (?:super::)*unsettled\(\)"), "|error| unsettled!(error)"),
    (re.compile(r"\|_\| \{\s*(?:super::)*unsettled\(\)\s*\}"), "|error| unsettled!(error)"),
    (re.compile(r"\b(?:super::)+unsettled\(\)"), "unsettled!()"),
    (re.compile(r"(?<![\w!:])(?<!fn )unsettled\(\)"), "unsettled!()"),
    # Passed as a function: wrap it so the macro sees the call site.
    (re.compile(r"\b(?:super::)*unsettled\)"), "|| unsettled!())"),
]

for path in FILES:
    text = path.read_text()
    new = text
    for pattern, replacement in RULES:
        new = pattern.sub(replacement, new)
    if new != text:
        path.write_text(new)
        print(path.relative_to(ROOT))
