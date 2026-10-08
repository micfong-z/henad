"""Unit tests for `docs_rs.py`. Run with `python3 -m unittest discover -s scripts`."""

from __future__ import annotations

import unittest

from docs_rs import DOCS_RS_TARGET, cargo_arguments, docs_rs_targets, published_crates


class TargetTests(unittest.TestCase):
    def test_a_table_without_targets_builds_the_docs_rs_target(self):
        self.assertEqual(docs_rs_targets({}), [DOCS_RS_TARGET])

    def test_the_first_named_target_is_the_default(self):
        table = {"targets": ["wasm32-unknown-unknown", DOCS_RS_TARGET]}
        self.assertEqual(docs_rs_targets(table), ["wasm32-unknown-unknown", DOCS_RS_TARGET])

    def test_a_default_target_comes_first_and_once(self):
        table = {"default-target": DOCS_RS_TARGET, "targets": ["wasm32-unknown-unknown", DOCS_RS_TARGET]}
        self.assertEqual(docs_rs_targets(table), [DOCS_RS_TARGET, "wasm32-unknown-unknown"])


class ArgumentTests(unittest.TestCase):
    def test_rustdoc_gets_docsrs_the_table_and_warnings_as_errors(self):
        arguments = cargo_arguments("henad", {"rustdoc-args": ["--cfg", "docsrs"]}, DOCS_RS_TARGET)
        self.assertEqual(
            arguments[:7], ["rustdoc", "--locked", "--package", "henad", "--lib", "--target", DOCS_RS_TARGET]
        )
        self.assertEqual(
            arguments[-2:],
            ["--config", 'build.rustdocflags=["--cfg", "docsrs", "--cfg", "docsrs", "-D", "warnings"]'],
        )

    def test_features_pass_through(self):
        table = {"features": ["app", "cli"], "all-features": True, "no-default-features": True}
        arguments = cargo_arguments("henad", table, DOCS_RS_TARGET)
        self.assertIn("app,cli", arguments)
        self.assertIn("--all-features", arguments)
        self.assertIn("--no-default-features", arguments)

    def test_rustc_args_reach_the_target_and_the_host(self):
        table = {"rustc-args": ["-C", "target-feature=+atomics"]}
        arguments = cargo_arguments("henad-app", table, "wasm32-unknown-unknown")
        self.assertIn('build.rustflags=["-C", "target-feature=+atomics"]', arguments)
        self.assertIn('host.rustflags=["-C", "target-feature=+atomics"]', arguments)
        self.assertIn("-Ztarget-applies-to-host", arguments)

    def test_no_rustc_args_set_no_rustflags(self):
        arguments = cargo_arguments("henad-core", {}, DOCS_RS_TARGET)
        self.assertFalse(any(argument.startswith("build.rustflags") for argument in arguments))


class CrateTests(unittest.TestCase):
    def test_every_published_crate_is_documented(self):
        names = {name for name, _ in published_crates()}
        for crate in ("core", "build", "compute", "models", "explore", "cli", "app"):
            self.assertIn(f"henad-{crate}", names)
        self.assertIn("henad", names)


if __name__ == "__main__":
    unittest.main()
