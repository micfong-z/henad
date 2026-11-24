#!/usr/bin/env python3
"""Test tiers of Henad's CI, and the scope of the jobs a change runs.

The quick tier runs on every push: every test target but the benchmarks, less the slow tests that `slow_tests.txt`
lists. The full tier runs every test target, nightly and on a dispatch.

    python3 scripts/ci_tiers.py test quick [ARGS...]   runs `cargo test` for a tier, ARGS passed to cargo
    python3 scripts/ci_tiers.py check                  fails when a line of slow_tests.txt matches no test, or several
    git diff --name-only A B | python3 scripts/ci_tiers.py scope
                                                       prints the jobs a change needs, as GITHUB_OUTPUT lines

Standard library only, so every runner can run it with the Python it ships.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

SLOW_TESTS = Path(__file__).with_name("slow_tests.txt")

# `--all-targets` without `--benches`. A benchmark runs once as a test under `cargo test`, and clippy compiles it.
QUICK_TARGETS = ["--lib", "--bins", "--tests", "--examples"]
FULL_TARGETS = ["--all-targets"]

# A line of `cargo test -- --list --format terse`, such as `tests::kit::a_test: test`.
LISTED_TEST = re.compile(r"^(?P<name>\S+): test$")

# Paths no job but the docs site reads. The tutorial pages are left out, since henad-tutorial's snippets test reads them.
DOCS_ONLY_FILES = {"AGENTS.md", "README.md", "zensical.toml", ".github/workflows/docs.yml"}
DOCS_ONLY_DIRS = ("docs/", "agents/")
TESTED_DOCS_DIRS = ("docs/guide/first-model/",)
PORTS_DIR = "benchmarks/"
MANIFEST = re.compile(r"^(Cargo\.(toml|lock)|crates/[^/]+/Cargo\.toml|examples/[^/]+/Cargo\.toml)$")


def read_slow_tests(text: str) -> list[str]:
    """Test paths of `slow_tests.txt`, without its comments and blank lines."""
    names = []
    for line in text.splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            names.append(line)
    return names


def test_command(tier: str, slow: list[str], extra: list[str]) -> list[str]:
    """The `cargo test` command of `tier`.

    `extra` holds arguments for cargo, then optionally `--` and arguments for the test harness, as `cargo test` takes
    them. The quick tier adds `--exact` and a `--skip` for each of `slow` to the harness's arguments. Note that `--exact`
    also makes a test name filter in `extra` match whole names.
    """
    cargo_args, harness_args = extra, []
    if "--" in extra:
        split = extra.index("--")
        cargo_args, harness_args = extra[:split], extra[split + 1 :]
    if tier == "quick":
        harness_args.append("--exact")
        for name in slow:
            harness_args += ["--skip", name]
    targets = QUICK_TARGETS if tier == "quick" else FULL_TARGETS
    command = ["cargo", "test", "--locked", "--workspace", *targets, "--all-features", *cargo_args]
    if harness_args:
        command += ["--", *harness_args]
    return command


def listed_tests(output: str) -> list[str]:
    """Names of the tests in the output of `cargo test -- --list --format terse`, over every test binary."""
    return [match["name"] for line in output.splitlines() if (match := LISTED_TEST.match(line.strip()))]


def slow_test_problems(tests: list[str], slow: list[str]) -> list[str]:
    """Problems with `slow` against the workspace's `tests`, empty when each line skips exactly one test.

    Under `--exact` the harness skips every test whose whole name is a `--skip` value, in every test binary. A line has to
    name exactly one test of the workspace.
    """
    problems = []
    seen = set()
    for name in slow:
        if name in seen:
            problems.append(f"'{name}' is listed twice")
            continue
        seen.add(name)
        matches = [test for test in tests if test == name]
        if not matches:
            problems.append(f"'{name}' matches no test. Rename the line with the test, or remove it")
        elif len(matches) > 1:
            problems.append(f"'{name}' names {len(matches)} tests, one in each of several crates")
    return problems


def scope(paths: list[str]) -> dict[str, bool]:
    """Jobs that a change to `paths` needs: `rust` for the workspace's jobs, `manifests` and `ports`."""
    rust = False
    for path in paths:
        tested_docs = path.startswith(TESTED_DOCS_DIRS)
        docs_only = (path in DOCS_ONLY_FILES or path.startswith(DOCS_ONLY_DIRS)) and not tested_docs
        if not docs_only and not path.startswith(PORTS_DIR):
            rust = True
    return {
        "rust": rust,
        "manifests": any(MANIFEST.match(path) for path in paths),
        "ports": any(path.startswith(PORTS_DIR) for path in paths),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    commands = parser.add_subparsers(dest="command", required=True)
    test = commands.add_parser("test", help="run cargo test for a tier")
    test.add_argument("tier", choices=["quick", "full"])
    test.add_argument("cargo_args", nargs=argparse.REMAINDER, help="arguments passed to cargo, such as --quiet")
    commands.add_parser("check", help="check slow_tests.txt against the workspace's tests")
    commands.add_parser("scope", help="read changed paths on stdin and print the jobs they need")
    args = parser.parse_args()

    slow = read_slow_tests(SLOW_TESTS.read_text(encoding="utf-8"))
    if args.command == "test":
        command = test_command(args.tier, slow, args.cargo_args)
        shown = command[: command.index("--")] if "--" in command else command
        note = f", skipping the {len(slow)} tests of slow_tests.txt" if args.tier == "quick" else ""
        print(f"{' '.join(shown)}{note}", flush=True)
        return subprocess.run(command, check=False).returncode
    if args.command == "check":
        listing = subprocess.run(
            ["cargo", "test", "--locked", "--workspace", *QUICK_TARGETS, "--all-features", "--", "--list",
             "--format", "terse"],
            check=True, capture_output=True, text=True,
        )
        tests = listed_tests(listing.stdout)
        problems = slow_test_problems(tests, slow)
        for problem in problems:
            print(f"slow_tests.txt: {problem}", file=sys.stderr)
        print(f"{len(tests)} tests: {len(tests) - len(slow)} in the quick tier, {len(slow)} in the full tier alone")
        return 1 if problems else 0
    for job, needed in scope([line.strip() for line in sys.stdin if line.strip()]).items():
        print(f"{job}={'true' if needed else 'false'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
