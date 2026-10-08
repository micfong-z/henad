"""Checks the frontmatter of every page under `docs/`. Run with `python3 -m unittest discover -s scripts`.

The site reads a page's metadata from its frontmatter, and a value YAML misreads fails without a sign. A plain
value holding `: `, or starting with an indicator YAML reserves such as a backtick or `@`, makes the block invalid,
and the page then shows its metadata as text. A plain value holding ` #` is cut at the `#`, which YAML reads as a
comment. None of these fails the docs build, strict mode included.
"""

from __future__ import annotations

import re
import unittest
from pathlib import Path

DOCS = Path(__file__).resolve().parent.parent / "docs"
RECORDS = DOCS / "developing" / "agent-record"
# Keys every session record carries, the three the site reads.
RECORD_KEYS = ("title", "description", "icon")

TOP_LEVEL = re.compile(r"^(\w+):(.*)$")
# Characters that cannot start a plain value: reserved, or read as an alias, an anchor, a tag or a directive.
INDICATORS = "`@*&!%"
# Starts that YAML reads as a sequence entry or a complex mapping key.
ENTRY_STARTS = ("- ", "? ")


def frontmatter(text: str) -> list[str] | None:
    """Returns the lines between a page's two `---` fences, or `None` for a page without frontmatter."""
    if not text.startswith("---\n"):
        return None
    end = text.find("\n---\n", 4)
    if end == -1:
        return None
    return text[4:end].split("\n")


def misread_values(lines: list[str]) -> list[str]:
    """Returns each top-level line whose plain value YAML would refuse, read as a mapping or cut at a comment."""
    misread = []
    for line in lines:
        match = TOP_LEVEL.match(line)
        if match is None:
            continue
        value = match.group(2).strip()
        if value[:1] in ('"', "'", "|", ">", "[", "{"):
            continue
        starts_badly = (value != "" and value[0] in INDICATORS) or value.startswith(ENTRY_STARTS)
        if starts_badly or ": " in value or " #" in value or value.endswith(":"):
            misread.append(line)
    return misread


class FrontmatterTests(unittest.TestCase):
    def test_no_plain_value_is_misread(self):
        for path in sorted(DOCS.rglob("*.md")):
            lines = frontmatter(path.read_text(encoding="utf-8"))
            if lines is None:
                continue
            with self.subTest(page=str(path.relative_to(DOCS))):
                self.assertEqual(misread_values(lines), [], "quote these values")

    def test_every_record_carries_the_keys_the_site_reads(self):
        for path in sorted(RECORDS.glob("*.md")):
            lines = frontmatter(path.read_text(encoding="utf-8")) or []
            keys = {match.group(1) for match in map(TOP_LEVEL.match, lines) if match is not None}
            with self.subTest(record=path.name):
                self.assertTrue(set(RECORD_KEYS) <= keys, f"missing {sorted(set(RECORD_KEYS) - keys)}")

    def test_a_plain_value_yaml_misreads_is_found(self):
        lines = [
            'title: "Quoted: fine"',
            "description: The first milestone of #48.",
            "delta_state: committed as `feat: CLI as a library`",
            "title: `henad-cli` as a library",
            "summary: @maintainer's notes",
            "status: - done",
            "issue: \"#48\"",
            "icon: material/robot",
            "tags:",
            'title: "`henad-cli` as a library"',
            "description: A crate named `henad-cli`, quoted where it starts a value.",
        ]
        self.assertEqual(misread_values(lines), lines[1:6])


if __name__ == "__main__":
    unittest.main()
