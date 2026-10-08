#!/usr/bin/env bash
# Builds the web target. Threads need nightly, a std rebuilt with atomics, and a shared wasm
# memory, none of which the pinned stable toolchain offers.
#
# Pass any trunk arguments through, e.g. `scripts/build_web.sh serve --release`.
set -euo pipefail

cd "$(dirname "$0")/.."

# One dated nightly, pinned in the template and read from there. Never installed from here.
toolchain="$(cat templates/model-project/scripts/web-toolchain)"
sysroot="$(RUSTUP_AUTO_INSTALL=0 rustup run "$toolchain" rustc --print sysroot 2>/dev/null || true)"
if [[ -z "$sysroot" || ! -d "$sysroot/lib/rustlib/src/rust/library" ]]; then
    echo "The web build needs $toolchain with rust-src. Install it with:" >&2
    echo "  rustup toolchain install \"$toolchain\" --profile minimal --component rust-src,clippy --target wasm32-unknown-unknown" >&2
    exit 1
fi
set -x

# rustc enables `target_thread_local` from `+atomics` but still links a private memory, so the
# memory flags and the TLS exports are asked for by hand. wasm-bindgen reads all of them, and a
# release build strips every name it does not have to keep.
flags=(
    -C target-feature=+atomics,+bulk-memory,+mutable-globals,+simd128
    -C link-arg=--shared-memory
    -C link-arg=--import-memory
    -C link-arg=--max-memory=4294967296
    -C link-arg=--export=__heap_base
    -C link-arg=--export=__wasm_init_tls
    -C link-arg=--export=__tls_size
    -C link-arg=--export=__tls_align
    -C link-arg=--export=__tls_base
)

# `CARGO_UNSTABLE_BUILD_STD` as an environment variable rather than a checked-in `[unstable]`
# block. A stray stable `cargo` in this workspace stays unaffected.
# An exported `CARGO_ENCODED_RUSTFLAGS` outranks `RUSTFLAGS` and would drop the flags above.
unset CARGO_ENCODED_RUSTFLAGS
RUSTUP_TOOLCHAIN="$toolchain" \
RUSTFLAGS="${flags[*]}" \
CARGO_UNSTABLE_BUILD_STD=std,panic_abort \
    trunk "${@:-build}"
