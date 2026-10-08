#!/usr/bin/env python3
"""Builds the documentation of each published crate as docs.rs builds it.

Each crate's `[package.metadata.docs.rs]` table gives the targets, the features and the flags, and they reach cargo
the way docs.rs passes them. The build runs on the nightly that `templates/model-project/scripts/web-toolchain`
names, and rustdoc gets `-D warnings` on top of the table's arguments.

    python3 scripts/docs_rs.py                 # every published crate, on each of its targets
    python3 scripts/docs_rs.py henad-explore   # the crates named
    python3 scripts/docs_rs.py --host          # the host in place of x86_64-unknown-linux-gnu

`--host` is for a machine that is no x86_64 Linux. A cross build to Linux needs its C libraries.
"""

import argparse
import json
import os
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Target docs.rs builds on, and the default target of a crate whose table names none.
DOCS_RS_TARGET = "x86_64-unknown-linux-gnu"

# Variables that outrank the flags this script passes through `--config`.
FLAG_VARIABLES = (
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "RUSTDOCFLAGS",
    "CARGO_ENCODED_RUSTDOCFLAGS",
    "CARGO_BUILD_RUSTDOCFLAGS",
)


def toml_array(values):
    """Returns `values` as a TOML array of strings, the form `--config` takes."""
    return "[" + ", ".join(json.dumps(value) for value in values) + "]"


def published_crates():
    """Returns the name and docs.rs table of every crate under `crates/` that publishes."""
    crates = []
    for manifest in sorted((ROOT / "crates").glob("*/Cargo.toml")):
        package = tomllib.loads(manifest.read_text())["package"]
        if package.get("publish", True) is False:
            continue
        table = package.get("metadata", {}).get("docs", {}).get("rs", {})
        crates.append((package["name"], table))
    return crates


def docs_rs_targets(table):
    """Returns the targets docs.rs builds for `table`, its default target first."""
    named = table.get("targets") or []
    default = table.get("default-target") or (named[0] if named else DOCS_RS_TARGET)
    return [default] + sorted(set(named) - {default})


def cargo_arguments(name, table, target):
    """Returns the arguments of `cargo` that document crate `name` for `target` as docs.rs does."""
    arguments = ["rustdoc", "--locked", "--package", name, "--lib", "--target", target]
    if "features" in table:
        arguments += ["--features", ",".join(table["features"])]
    if table.get("all-features"):
        arguments.append("--all-features")
    if table.get("no-default-features"):
        arguments.append("--no-default-features")
    rustc_args = table.get("rustc-args") or []
    if rustc_args:
        # docs.rs hands the flags to host dependencies, build scripts and proc macros, as well.
        arguments += ["--config", f"build.rustflags={toml_array(rustc_args)}"]
        arguments += ["-Zhost-config", "-Ztarget-applies-to-host"]
        arguments += ["--config", f"host.rustflags={toml_array(rustc_args)}"]
    rustdoc_args = ["--cfg", "docsrs", *table.get("rustdoc-args", []), "-D", "warnings"]
    arguments += ["--config", f"build.rustdocflags={toml_array(rustdoc_args)}"]
    return arguments


def host_target(environment):
    """Returns the triple of the host the toolchain in `environment` runs on."""
    output = subprocess.run(
        ["rustc", "-vV"], env=environment, check=True, capture_output=True, text=True
    ).stdout
    return next(line.split(":", 1)[1].strip() for line in output.splitlines() if line.startswith("host:"))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("crates", nargs="*", help="crates to document, every published crate when none is named")
    parser.add_argument("--host", action="store_true", help=f"document the host in place of {DOCS_RS_TARGET}")
    options = parser.parse_args()

    environment = {key: value for key, value in os.environ.items() if key not in FLAG_VARIABLES}
    environment["RUSTUP_TOOLCHAIN"] = (ROOT / "templates/model-project/scripts/web-toolchain").read_text().strip()
    # Set for the build scripts of every docs.rs build.
    environment["DOCS_RS"] = "1"
    native = host_target(environment) if options.host else DOCS_RS_TARGET

    crates = published_crates()
    unknown = sorted(set(options.crates) - {name for name, _ in crates})
    if unknown:
        parser.error(f"no published crate is named {', '.join(unknown)}")

    grouped = os.environ.get("GITHUB_ACTIONS") == "true"
    failures = []
    for name, table in crates:
        if options.crates and name not in options.crates:
            continue
        for target in docs_rs_targets(table):
            build_target = native if target == DOCS_RS_TARGET else target
            # Cargo keeps each target's builds apart, and a crate with flags of its own takes a directory of its own.
            directory = ROOT / "target" / "docs-rs"
            environment["CARGO_TARGET_DIR"] = str(directory / name if table.get("rustc-args") else directory)
            command = ["cargo", *cargo_arguments(name, table, build_target)]
            # A collapsed group per build in a GitHub Actions log.
            if grouped:
                print(f"::group::{name} on {build_target}", flush=True)
            print(" ".join(command), flush=True)
            result = subprocess.run(command, cwd=ROOT, env=environment)
            if grouped:
                print("::endgroup::", flush=True)
            if result.returncode != 0:
                failures.append(f"{name} on {build_target}")

    for failure in failures:
        print(f"docs.rs build failed: {failure}", file=sys.stderr)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
