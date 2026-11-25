"""Unit tests for `ci_tiers.py`. Run with `python3 -m unittest discover -s scripts`."""

from __future__ import annotations

import unittest
from pathlib import Path

from ci_tiers import (
    FULL_TARGETS,
    QUICK_TARGETS,
    SLOW_TESTS,
    listed_tests,
    read_slow_tests,
    scope,
    slow_test_problems,
    test_command,
)


class SlowTestsTest(unittest.TestCase):
    def test_comments_and_blank_lines_are_left_out(self) -> None:
        text = "# a comment\n\ntests::a::one\n  tests::b::two  \n# tests::c::three\n"
        self.assertEqual(read_slow_tests(text), ["tests::a::one", "tests::b::two"])

    def test_the_committed_list_reads(self) -> None:
        slow = read_slow_tests(Path(SLOW_TESTS).read_text(encoding="utf-8"))
        self.assertTrue(slow)
        self.assertEqual(len(slow), len(set(slow)))

    def test_a_line_matching_one_test_is_fine(self) -> None:
        tests = ["tests::kit::a_slow_check", "tests::kit::a_quick_check"]
        self.assertEqual(slow_test_problems(tests, ["tests::kit::a_slow_check"]), [])

    def test_a_line_matching_no_test_is_reported(self) -> None:
        problems = slow_test_problems(["tests::kit::renamed"], ["tests::kit::old_name"])
        self.assertEqual(len(problems), 1)
        self.assertIn("matches no test", problems[0])

    def test_a_prefix_of_another_name_matches_its_own_test_alone(self) -> None:
        tests = ["tests::kit::a_check", "tests::kit::a_check_on_the_host"]
        self.assertEqual(slow_test_problems(tests, ["tests::kit::a_check"]), [])

    def test_a_line_naming_a_test_of_two_crates_is_reported(self) -> None:
        # Two crates can hold the same path, and `--skip` would skip it in both.
        problems = slow_test_problems(["tests::kit::a_check", "tests::kit::a_check"], ["tests::kit::a_check"])
        self.assertEqual(len(problems), 1)
        self.assertIn("names 2 tests", problems[0])

    def test_a_line_listed_twice_is_reported(self) -> None:
        problems = slow_test_problems(["tests::kit::a"], ["tests::kit::a", "tests::kit::a"])
        self.assertEqual(problems, ["'tests::kit::a' is listed twice"])


class ListingTest(unittest.TestCase):
    def test_tests_are_read_and_benchmarks_and_summaries_left_out(self) -> None:
        output = "tests::a::one: test\ntests::b::two: test\nscatter/sort: benchmark\n\n2 tests, 1 benchmark\n"
        self.assertEqual(listed_tests(output), ["tests::a::one", "tests::b::two"])


class CommandTest(unittest.TestCase):
    def test_the_quick_tier_skips_each_slow_test_and_the_benchmarks(self) -> None:
        command = test_command("quick", ["tests::a", "tests::b"], ["--quiet"])
        self.assertEqual(command[:4], ["cargo", "test", "--locked", "--workspace"])
        for target in QUICK_TARGETS:
            self.assertIn(target, command)
        self.assertNotIn("--all-targets", command)
        self.assertEqual(
            command[command.index("--") :], ["--", "--exact", "--skip", "tests::a", "--skip", "tests::b"]
        )
        self.assertLess(command.index("--quiet"), command.index("--"))

    def test_the_full_tier_runs_every_target_and_skips_nothing(self) -> None:
        command = test_command("full", ["tests::a"], [])
        for target in FULL_TARGETS:
            self.assertIn(target, command)
        self.assertNotIn("--skip", command)
        self.assertNotIn("--", command)

    def test_harness_arguments_stay_after_the_separator(self) -> None:
        command = test_command("quick", ["tests::a"], ["--quiet", "--", "--nocapture"])
        self.assertEqual(command.count("--"), 1)
        self.assertEqual(command[command.index("--") :], ["--", "--nocapture", "--exact", "--skip", "tests::a"])


class ScopeTest(unittest.TestCase):
    def test_a_docs_change_runs_no_workspace_job(self) -> None:
        self.assertEqual(
            scope(["docs/guide/app.md", "agents/ui.md", "README.md", "zensical.toml"]),
            {"rust": False, "manifests": False, "ports": False},
        )

    def test_a_tutorial_page_runs_the_workspace_jobs(self) -> None:
        self.assertTrue(scope(["docs/guide/first-model/ants.md"])["rust"])

    def test_a_source_change_runs_the_workspace_jobs(self) -> None:
        self.assertTrue(scope(["docs/guide/app.md", "crates/henad-app/src/lib.rs"])["rust"])

    def test_the_changelog_and_scripts_run_the_workspace_jobs(self) -> None:
        # The lint job runs the scripts' tests, and they read the changelog.
        self.assertTrue(scope(["CHANGELOG.md"])["rust"])
        self.assertTrue(scope(["scripts/docs_rs.py"])["rust"])

    def test_a_port_change_runs_the_ports_job_alone(self) -> None:
        self.assertEqual(
            scope(["benchmarks/mesa/models/sir.py"]), {"rust": False, "manifests": False, "ports": True}
        )

    def test_a_manifest_change_is_seen(self) -> None:
        self.assertTrue(scope(["crates/henad-core/Cargo.toml"])["manifests"])
        self.assertTrue(scope(["Cargo.lock"])["manifests"])
        self.assertFalse(scope(["crates/henad-core/src/lib.rs"])["manifests"])

    def test_no_change_runs_nothing(self) -> None:
        self.assertEqual(scope([]), {"rust": False, "manifests": False, "ports": False})


if __name__ == "__main__":
    unittest.main()
