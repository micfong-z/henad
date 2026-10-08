#!/usr/bin/env bash
# This scripts runs various CI-like checks in a convenient way.
set -eux

cargo check --quiet --workspace --all-targets
# Without atomics. henad-app then has no thread pool, as docs.rs builds it.
cargo check --quiet -p henad-core -p henad-compute -p henad-models -p henad-explore -p henad-app --all-features --lib --target wasm32-unknown-unknown
# The facade with no feature and with all four. henad-cli sits behind a native target gate and never joins.
cargo check --quiet -p henad --lib --target wasm32-unknown-unknown
cargo check --quiet -p henad --all-features --lib --target wasm32-unknown-unknown
cargo fmt --all -- --check
./scripts/check_packaging.sh
# CI runs cargo-deny on every pull request, and a machine without it skips the check here.
if cargo deny --version >/dev/null 2>&1; then
    cargo deny --log-level error --locked check
fi
cargo clippy --quiet --workspace --all-targets --all-features --  -D warnings -W clippy::all
cargo test --quiet --workspace --all-targets --all-features
cargo test --quiet --workspace --doc
# The README program, which the workspace's doc tests leave out with both of the facade's features off.
cargo test --quiet -p henad --doc --features example-models,app
RUSTDOCFLAGS="-D warnings" cargo doc --quiet --workspace --no-deps --all-features
python3 -m unittest discover -s scripts -p 'test_*.py'
./scripts/build_web.sh build
