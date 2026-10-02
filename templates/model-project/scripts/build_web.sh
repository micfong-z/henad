#!/usr/bin/env bash
# Builds the web app with Trunk. Threads need a dated nightly and a std rebuilt with atomics, and the flags for both
# live in `.cargo/config.toml`.
#
# Pass any Trunk arguments through, as in `scripts/build_web.sh serve --release`.
set -euo pipefail

cd "$(dirname "$0")/.."

# Either variable replaces the flags in `.cargo/config.toml`, and the build would then stop at a `compile_error!`
# that names neither.
if [[ -n "${RUSTFLAGS+set}" || -n "${CARGO_ENCODED_RUSTFLAGS+set}" ]]; then
    echo "RUSTFLAGS or CARGO_ENCODED_RUSTFLAGS is set, and either would replace the wasm flags in .cargo/config.toml." >&2
    echo "Unset both for this build. A wasm-only flag of your own goes into the rustflags array of .cargo/config.toml." >&2
    exit 1
fi

# The dated nightly, never installed from here.
toolchain="$(cat scripts/web-toolchain)"
sysroot="$(RUSTUP_AUTO_INSTALL=0 rustup run "$toolchain" rustc --print sysroot 2>/dev/null || true)"
if [[ -z "$sysroot" || ! -d "$sysroot/lib/rustlib/src/rust/library" ]]; then
    echo "The web build needs $toolchain with rust-src. Install it with:" >&2
    echo "  rustup toolchain install \"$toolchain\" --profile minimal --component rust-src,clippy --target wasm32-unknown-unknown" >&2
    exit 1
fi

RUSTUP_TOOLCHAIN="$toolchain" \
CARGO_UNSTABLE_BUILD_STD=std,panic_abort \
    trunk "${@:-build}"
