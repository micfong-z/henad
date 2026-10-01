#!/usr/bin/env bash
# Checks that every crate packages on its own. The workspace versions agree, each crate carries the
# licence texts, and no source or build script reads a file outside its crate. Only a verified
# `cargo package` builds a crate from its tarball, and the release checklist runs it by hand.
set -euo pipefail

cd "$(dirname "$0")/.."

python3 - <<'EOF'
import re
import sys
import tomllib
from pathlib import Path

errors = []

# Every Henad crate requires its siblings at the workspace version, as a caret requirement.
root = tomllib.loads(Path("Cargo.toml").read_text())
version = root["workspace"]["package"]["version"]
for name, spec in root["workspace"]["dependencies"].items():
    if not name.startswith("henad"):
        continue
    if not isinstance(spec, dict) or spec.get("version") != version:
        errors.append(f"Cargo.toml: `{name}` needs `version = \"{version}\"` beside its path")

crates = sorted(path.parent for path in Path("crates").glob("*/Cargo.toml"))

# Each package ships both licence texts, as copies of the root's.
for crate in crates:
    for licence in ("LICENSE-MIT", "LICENSE-APACHE"):
        copy = crate / licence
        if not copy.is_file() or copy.read_bytes() != Path(licence).read_bytes():
            errors.append(f"{copy}: needs to be a copy of the root {licence}")

# An `include_str!` or `include_bytes!` path resolves inside its own crate.
include = re.compile(r'include_(?:str|bytes)!\(\s*"([^"]+)"')
for crate in crates:
    for source in crate.rglob("*.rs"):
        for number, line in enumerate(source.read_text().splitlines(), 1):
            for path in include.findall(line):
                target = (source.parent / path).resolve()
                if not target.is_relative_to(crate.resolve()):
                    errors.append(f"{source}:{number}: `{path}` is outside {crate}")

# A build script joins no path that climbs out of its crate. Listed here are the build scripts that
# read the shared WGSL from henad-compute's sources.
climbing_allowed = {Path("crates/henad-models/build.rs")}
for crate in crates:
    script = crate / "build.rs"
    if not script.is_file() or script in climbing_allowed:
        continue
    for number, line in enumerate(script.read_text().splitlines(), 1):
        if '"../' in line or '".."' in line:
            errors.append(f"{script}:{number}: a path that climbs out of {crate}")

for error in errors:
    print(error, file=sys.stderr)
sys.exit(1 if errors else 0)
EOF
