---
title: Contribution Guidelines
description: A guide to contributing to Henad
icon: material/source-pull
---

# Contribution Guidelines

Thank you for considering to contribute to Henad!
However Henad is in very early stages so expect everything to change.
It would be best if you could create an issue first to discuss what you want to contribute before making a PR.

## Issues

You can [create an issue](https://github.com/micfong-z/henad/issues/new) to report a bug or to request a feature.
Please try to search for existing issues to avoid duplicates before creating a new one.

For questions or help about Henad, please use [GitHub Discussion](https://github.com/micfong-z/henad/discussions) instead.

## Making a PR

Go ahead and open a PR if you want to contribute small changes.
For larger changes, please create an issue first to discuss your ideas to avoid duplicate work!

You can test your code with `./check.sh`.
Its last step builds the web app, which needs the dated nightly that `templates/model-project/scripts/web-toolchain` specifies, with `rust-src`:

```bash
rustup toolchain install "$(cat templates/model-project/scripts/web-toolchain)" --profile minimal \
  --component rust-src,clippy --target wasm32-unknown-unknown
```

`./check.sh` also runs `cargo deny` when [cargo-deny](https://github.com/EmbarkStudios/cargo-deny) is installed.
CI runs it on every pull request, together with checks that `./check.sh` leaves out, among them a `cargo package` pass when a manifest or `Cargo.lock` changes, a check at the minimum supported Rust version, a check of the `henad` crate with each feature on its own, the build of the reference ports, and each published crate's documentation as docs.rs builds it, through `scripts/docs_rs.py`.
Neither `./check.sh` nor the `cargo package` pass builds a crate from its tarball.
The release checklist does.
`.github/workflows/ci.yml` lists every CI check.

## AI usage

This project is assisted by AI, though every line of code generated has been reviewed (and almost always, edited) by a human.

We recognise that LLM-assisted coding is evolving increasingly rapidly, but the quality of the code generated is not always guaranteed.
In general, we support the [LLVM AI Tool Use Policy](https://llvm.org/docs/AIToolPolicy.html) for coding (and for some documentation), but discourage the use of AI for communication except purely for translation.

All agent coding sessions since 2026-08-12 are auto-documented in docs/developing/agent-record, along with human comments.
