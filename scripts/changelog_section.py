#!/usr/bin/env python3
"""Prints one version's section of `CHANGELOG.md`, for use as a release body.

Standard library only. The release workflow runs it with a bare `python3`, without the
`uv --project scripts` environment the analysis scripts need.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

# `## [0.1.0] - 2026-09-08`, the shape Keep a Changelog uses.
HEADING = re.compile(r"^##\s+\[(?P<version>[^\]]+)\]\s*(?P<rest>.*)$")
DATE = re.compile(r"^-\s*\d{4}-\d{2}-\d{2}$")
# `[0.1.0]: https://...`, the block of link definitions at the foot of the file.
LINK_DEFINITION = re.compile(r"^\[[^\]]+\]:\s")


class ChangelogError(Exception):
    """A changelog the release workflow refuses to draw notes from."""


def section(text: str, version: str) -> str:
    """The body under `## [version]`, with the heading and the trailing link block removed."""
    preceding: list[str] = []
    body: list[str] = []
    heading: re.Match[str] | None = None

    for line in text.splitlines():
        match = HEADING.match(line)
        if match:
            if heading is not None:
                break
            preceding.append(match["version"])
            if match["version"] == version:
                heading = match
        elif heading is not None:
            body.append(line)

    if heading is None:
        known = ", ".join(preceding) or "nothing"
        raise ChangelogError(f"no `## [{version}]` section. The file has: {known}.")

    # An unreleased section would ship notes for a version nobody dated.
    if not DATE.match(heading["rest"].strip()):
        raise ChangelogError(
            f"`{heading.group(0).strip()}` carries no release date. "
            f"Write it as `## [{version}] - YYYY-MM-DD` before tagging."
        )

    while body and (not body[-1].strip() or LINK_DEFINITION.match(body[-1])):
        body.pop()
    while body and not body[0].strip():
        body.pop(0)

    if not body:
        raise ChangelogError(f"`## [{version}]` is empty.")

    return "\n".join(body) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version", help="the version to extract, with or without a leading `v`")
    parser.add_argument("--changelog", type=Path, default=Path("CHANGELOG.md"))
    args = parser.parse_args()

    try:
        text = args.changelog.read_text(encoding="utf-8")
        sys.stdout.write(section(text, args.version.removeprefix("v")))
    except (ChangelogError, OSError) as error:
        print(f"{args.changelog}: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
