#!/usr/bin/env bash
# This scripts runs various CI-like checks in a convenient way.
set -eux

cargo check --quiet --workspace --all-targets
# Everything under henad-app still typechecks without atomics. henad-app itself cannot, since
# `wasm-bindgen-rayon` refuses to compile without them.
cargo check --quiet -p henad-core -p henad-compute -p henad-models --all-features --lib --target wasm32-unknown-unknown
cargo fmt --all -- --check
cargo clippy --quiet --workspace --all-targets --all-features --  -D warnings -W clippy::all
cargo test --quiet --workspace --all-targets --all-features
cargo test --quiet --workspace --doc
./scripts/build_web.sh build
