#!/usr/bin/env bash
# Download mason.22.jar next to this script and check its digest.
#
# The jar is not committed. `scripts/compare_bench.py` looks for it at $MASON_JAR, else here.
# Set MASON_URL to fetch from a mirror.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
jar="$here/mason.22.jar"
url="${MASON_URL:-https://cs.gmu.edu/~eclab/projects/mason/mason.22.jar}"
digest="e9726d0fc049090ea7d0105e5e4b130abcad7eb32a7d8c0e54d1d33016e9e3d8"

check() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d' ' -f1
    else
        sha256sum "$1" | cut -d' ' -f1
    fi
}

if [ -f "$jar" ] && [ "$(check "$jar")" = "$digest" ]; then
    echo "mason.22.jar is already here and matches."
    exit 0
fi

echo "fetching $url"
curl --fail --location --progress-bar --output "$jar.part" "$url"

got="$(check "$jar.part")"
if [ "$got" != "$digest" ]; then
    rm -f "$jar.part"
    echo "digest mismatch: expected $digest, got $got" >&2
    exit 1
fi
mv "$jar.part" "$jar"
echo "wrote $jar"
