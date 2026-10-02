#!/usr/bin/env bash
# Runs the checks CI runs, one stage per argument, and all four without one.
#
#   lint      clippy over the package, warnings denied
#   lint-web  clippy for the threaded web build, on the nightly in `scripts/web-toolchain`
#   test      the tests, among them the checks of every model in `models()`
#   web       the release web build, then a check that `dist/` holds the worker glue
#
# Each stage passes `--locked` once a `Cargo.lock` exists.
set -euo pipefail

cd "$(dirname "$0")/.."

locked=()
if [[ -f Cargo.lock ]]; then
    locked=(--locked)
fi

stage_lint() {
    cargo clippy ${locked[@]+"${locked[@]}"} --all-targets --all-features -- -D warnings
}

stage_lint_web() {
    if [[ -n "${RUSTFLAGS+set}" || -n "${CARGO_ENCODED_RUSTFLAGS+set}" ]]; then
        echo "RUSTFLAGS or CARGO_ENCODED_RUSTFLAGS is set, and either would replace the wasm flags in .cargo/config.toml." >&2
        exit 1
    fi
    # A target directory of its own. The nightly would otherwise rebuild what the stable stages built.
    RUSTUP_TOOLCHAIN="$(cat scripts/web-toolchain)" \
    CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target}/lint-web" \
        cargo clippy ${locked[@]+"${locked[@]}"} --all-features --target wasm32-unknown-unknown -- -D warnings
}

stage_test() {
    cargo test ${locked[@]+"${locked[@]}"} --all-targets --all-features
    cargo test ${locked[@]+"${locked[@]}"} --doc --all-features
}

stage_web() {
    scripts/build_web.sh build --release ${locked[@]+"${locked[@]}"}
    # The worker pool starts from glue that wasm-bindgen-rayon exports, and a build without it still succeeds.
    local binary
    binary="$(sed -n 's/.*data-bin="\([^"]*\)".*/\1/p' index.html)"
    grep -q wbg_rayon_start_worker "dist/$binary.js"
    grep -q initThreadPool "dist/$binary.js"
    ls dist/snippets/wasm-bindgen-rayon-*/src/workerHelpers.no-bundler.js
}

stages=("$@")
if [[ $# -eq 0 ]]; then
    stages=(lint lint-web test web)
fi
for stage in "${stages[@]}"; do
    echo "--- $stage" >&2
    case "$stage" in
        lint) stage_lint ;;
        lint-web) stage_lint_web ;;
        test) stage_test ;;
        web) stage_web ;;
        *)
            echo "Unknown stage '$stage'. The stages are lint, lint-web, test and web." >&2
            exit 2
            ;;
    esac
done
