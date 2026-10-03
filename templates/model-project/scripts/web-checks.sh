# shellcheck shell=bash
# The checks every web stage runs first, sourced from the project root by `build_web.sh` and `ci.sh`.

# Prints the dated nightly in `scripts/web-toolchain`, or explains on stderr what is missing and fails.
#
# Either flag variable replaces the flags in `.cargo/config.toml`, and the build would then stop at a `compile_error!`
# that names neither. The nightly is never installed from here.
web_toolchain() {
    if [[ -n "${RUSTFLAGS+set}" || -n "${CARGO_ENCODED_RUSTFLAGS+set}" ]]; then
        echo "RUSTFLAGS or CARGO_ENCODED_RUSTFLAGS is set, and either would replace the wasm flags in .cargo/config.toml." >&2
        echo "Unset both for this build. A wasm-only flag of your own goes into the rustflags array of .cargo/config.toml." >&2
        return 1
    fi

    local toolchain sysroot
    toolchain="$(cat scripts/web-toolchain)"
    sysroot="$(RUSTUP_AUTO_INSTALL=0 rustup run "$toolchain" rustc --print sysroot 2>/dev/null || true)"
    if [[ -z "$sysroot" || ! -d "$sysroot/lib/rustlib/src/rust/library" ||
        ! -d "$sysroot/lib/rustlib/wasm32-unknown-unknown" ]]; then
        echo "The web build needs $toolchain with rust-src and the wasm32 target. Install it with:" >&2
        echo "  rustup toolchain install \"$toolchain\" --profile minimal --component rust-src,clippy --target wasm32-unknown-unknown" >&2
        return 1
    fi
    echo "$toolchain"
}
