"""Unit tests for `changelog_section.py`. Run with `python3 -m unittest discover -s scripts`."""

from __future__ import annotations

import unittest

from changelog_section import ChangelogError, section

CHANGELOG = """# Changelog

## [Unreleased]

## [0.2.0] - 2026-10-01

### Added

- A second thing.

## [0.1.0] - 2026-09-08

First release.

### Engine

- A thing.

[Unreleased]: https://example.invalid/compare/v0.2.0...HEAD
[0.2.0]: https://example.invalid/releases/tag/v0.2.0
[0.1.0]: https://example.invalid/releases/tag/v0.1.0
"""


class SectionTests(unittest.TestCase):
    def test_stops_at_the_next_version(self):
        body = section(CHANGELOG, "0.2.0")
        self.assertIn("A second thing.", body)
        self.assertNotIn("First release.", body)

    def test_the_last_section_drops_the_link_block(self):
        body = section(CHANGELOG, "0.1.0")
        self.assertEqual(body, "First release.\n\n### Engine\n\n- A thing.\n")

    def test_an_unknown_version_names_the_ones_there_are(self):
        with self.assertRaises(ChangelogError) as caught:
            section(CHANGELOG, "9.9.9")
        self.assertIn("0.1.0", str(caught.exception))

    # The date is the only thing separating a tagged version from a section still being written.
    def test_an_undated_version_is_refused(self):
        with self.assertRaises(ChangelogError) as caught:
            section("## [0.1.0] - Unreleased\n\nNotes.\n", "0.1.0")
        self.assertIn("release date", str(caught.exception))

    def test_a_bare_heading_is_refused(self):
        with self.assertRaises(ChangelogError):
            section("## [0.1.0]\n\nNotes.\n", "0.1.0")

    def test_an_empty_section_is_refused(self):
        with self.assertRaises(ChangelogError) as caught:
            section("## [0.1.0] - 2026-09-08\n\n## [0.0.9] - 2026-09-01\n\nOld.\n", "0.1.0")
        self.assertIn("empty", str(caught.exception))


if __name__ == "__main__":
    unittest.main()
