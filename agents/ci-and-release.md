# CI and releases

What CI runs beyond `./check.sh`, the release procedure, the licence page and the toolchain pins.

## CI jobs

`./check.sh` runs every line of the command block in `AGENTS.md` but the install, and skips
`cargo deny` where cargo-deny is not installed. CI alone runs `cargo package --workspace --exclude
henad-tutorial --no-verify --locked` (the `package` job, on manifest changes), `cargo +1.95 check
--workspace --locked` and the facade with every feature on 1.95 (`msrv`), `cargo check -p henad`
with no features and then with each feature alone (`features`), clippy on the pinned nightly for
wasm32 with `RUSTFLAGS="-C target-feature=+atomics,+bulk-memory"` and a `CARGO_TARGET_DIR` of its
own, over `-p henad-app` and over `-p henad --features app,example-models` (`lint`), `actionlint`
over `templates/model-project/.github/workflows/*.yml` (`lint`), and every published crate's docs
as docs.rs builds them, through `scripts/docs_rs.py` from each crate's `[package.metadata.docs.rs]`
table on the pinned nightly (`docs`). `msrv`, `features` and `docs` run on a pull request and on a
dispatch, and `package` on a pull request alone. The `downstream` job runs on `workflow_dispatch`
alone, with `CARGO_TERM_COLOR=never`, and the release checklist starts it. It runs the verified
packaging in two fresh target directories, the second with the facade's features, and deletes both
builds, copies the template outside the checkout, pins `henad` and `henad-build` to `=<version>`
(three lines), patches every crate it uses to the unpacked tarballs under the verify directory's
`package/`, and generates a lock.
Then, in order: both `henad` and `henad-build` resolve with a null `source`, henad-models is
absent, strict clippy (`-D unreachable_pub -D unused_qualifications`), `scripts/ci.sh test` under
`HENAD_REQUIRE_GPU=1`, a second `cargo build -v` with nothing `Dirty`, `Compiling` or `Running`, a
stable wasm32 check of the library with `--no-default-features` and `RUSTFLAGS=""`, `ci.sh
lint-web`, `build_web.sh` refusing `RUSTFLAGS`, `ci.sh web`, a comment appended to `src/vote.rs`
recompiling no henad crate, an unreferenced shader added under `src/probe/` compiling under
`-D warnings`, the CLI's dry run of `specs/vote.toml`, and the CPU-only crate (`gpu_vote`, its
lines and the `ShaderBuild` line removed) sweeping and resuming with no `build_changed` warning.
The wasm32 steps run before any step changes the copy. To run it by hand, run each step's script
from `ci.yml` with `RUNNER_TEMP` set to a directory outside any git work tree and
`CARGO_TERM_COLOR=never`, and pass `--allow-dirty` to `cargo package` on an uncommitted tree.

## Releases

Preview a release's notes: `python3 scripts/changelog_section.py 0.1.0` (what the tag build puts in
the release body; it refuses a section with no date)
Cut a release: `docs/developing/releasing.md` holds the order (bump the version, every requirement
and the template's three, and on a breaking release the facade README's line, the guide's
admonitions and its typed dependency snippets, regenerate `docs/license.html`, `cargo update
--workspace`, date the CHANGELOG, commit and push, run the checklist on that commit, then tag), the
checklist the maintainer copies into an issue per release (both verified packaging passes in a
fresh `CARGO_TARGET_DIR`, the second with the facade's features, `gh workflow run ci.yml --ref
<branch>` for the `downstream`, `msrv`, `features` and `docs` jobs, a native check on the pinned
nightly, `cargo semver-checks`, a WGSL diff and a review of the hidden items another crate calls on
a patch release, `cargo publish --workspace --dry-run --locked` in a fresh `CARGO_TARGET_DIR`), the
paced publish and the stability policy. The maintainer runs every publish by hand. Never tag,
publish or bump the version unasked.

## Third-party licence page

Regenerate the third-party licence page: `cargo about generate about.hbs -o docs/license.html`
(needs `cargo-about`, pinned to 0.9.1 in CI; the `lint` job fails when the committed page and the
dependency tree disagree). A crate shipping two files under one licence gets a `clarify` entry in
`about.toml`, as `rfd` and `miniz_oxide` have. Left alone, cargo-about keeps whichever file its
directory walk finds first, and walk order depends on the filesystem. A page generated on macOS can
then fail the check on Linux, or pass it by luck, as `miniz_oxide` did. The app's four fonts are no
crates, and their section of the page is static HTML in `about.hbs`. Their sources, licences and the
procedure that rebuilds Henad Sans and Henad Mono from IBM Plex are in
`crates/henad-app/assets/fonts/SOURCES.md`.

## Toolchain

Toolchain is pinned via `rust-toolchain` (1.97, with rustfmt/clippy/wasm32-unknown-unknown target).
The web build is the exception and runs on one dated nightly, named in
`templates/model-project/scripts/web-toolchain` and selected by `scripts/build_web.sh`, which prints
the install line in `AGENTS.md` when the toolchain or its `rust-src` is missing. The Trunk release sits beside
it in `templates/model-project/scripts/trunk-version`. CI, vercel.json and the docs read both files,
and the repository holds one pin. `trunk-sha256` beside them holds the SHA-256 of that release's Linux
x86_64 tarball, which Henad's CI and the template's both check before extracting it, so a Trunk bump
changes both files. Threads on wasm need
`-C target-feature=+atomics,+bulk-memory,+mutable-globals` and a std rebuilt to match, so the nightly
needs `rust-src`. That script is the only supported way to build for the web; a bare `trunk build`
produces a binary whose thread pool cannot start. It also unsets `CARGO_ENCODED_RUSTFLAGS`, which
would otherwise outrank the `RUSTFLAGS` it sets.
