#!/usr/bin/env bash
# Builds the web app with Trunk. Threads need a dated nightly and a std rebuilt with atomics, and the flags for both
# live in `.cargo/config.toml`.
#
# Pass any Trunk arguments through, as in `scripts/build_web.sh serve --release`.
set -euo pipefail

cd "$(dirname "$0")/.."

# shellcheck source=scripts/web-checks.sh
source scripts/web-checks.sh
toolchain="$(web_toolchain)"

trunk_version="$(cat scripts/trunk-version)"
if ! command -v trunk > /dev/null; then
    echo "The web build needs Trunk $trunk_version. Install it with:" >&2
    echo "  cargo install --locked trunk --version $trunk_version" >&2
    exit 1
fi

RUSTUP_TOOLCHAIN="$toolchain" \
CARGO_UNSTABLE_BUILD_STD=std,panic_abort \
    trunk "${@:-build}"
