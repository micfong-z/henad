# henad-build

**henad-build**: a build dependency, on henad-core, `wgsl_bindgen` and `heck` alone, the last
naming a `ShaderEntry` variant as `wgsl_bindgen` does. `lib.rs` holds `ShaderBuild` and
`ShaderBuildError`, `paths.rs` the walk of a shader root and the Rust names a path becomes
(`check_components`, `check_reserved`, `check_collisions`, `check_module_name`),
`binding_lines.rs` the private reader of `@group(0)` lines, and `output.rs` the shared-module
copies, the files the entry points import (`imported_files`, through `wgsl_bindgen`'s
`DependencyTree`), the stamp and the two generated files. `stamp/` holds the build stamps:
`stamp_commit` for a host or model crate, and the hidden `stamp_engine_commit` (henad-explore)
and `stamp_source_hash` (henad-compute, henad-models). `stamp/mod.rs` picks the source of a
stamp by `StampScope`: a package's `.cargo_vcs_info.json` first, then git when
`git ls-files --error-unmatch Cargo.toml` succeeds, and for the engine only when `--show-prefix`
is `crates/henad-explore/`. `stamp/files.rs` lists and hashes the files under `src` and the
manifest (`Cargo.toml.orig` in a package, which holds a `.cargo_vcs_info.json` or a `Cargo.toml`
Cargo generated), with the nearest `Cargo.lock` outside a package, CRLF read as LF, and a file
that vanishes mid-read skipped. Dotfiles, `~`, `.swp`, `.orig` and `.rej` files, `#name#`
autosaves (`is_excluded`) and symlinks to a directory or to nothing (`is_link_to_no_file`) are
left out of the hash and the dirty flag alike. `stamp/git.rs` asks git, with
`GIT_OPTIONAL_LOCKS=0`, for the commit (the full hash cut to eight characters), its date
(`log.showSignature` off, and empty unless it reads `YYYY-MM-DD`), the dirty flag
(`status --porcelain -z --untracked-files=all`, over pathspecs built from `--show-prefix`, and
unknown in place of clean when git does not track the lockfile) and the paths a commit changes,
resolved through `--git-path` and `--git-common-dir`, so packed refs and a linked worktree stay
watched. A crate in a repository that does not track its manifest yet watches those paths too,
and its first commit reruns the stamp. A git before 2.31 echoes `--path-format=absolute` back,
and the paths are then joined onto the crate's directory (`echoes_path_format`). In Henad's
checkout the engine's hash covers henad-core, henad-build, henad-compute and henad-explore with
the lockfile, and `HENAD_BUILD_CRATE_HASH` covers henad-explore alone.
`scripts/check_packaging.sh` refuses a `"../` path in `stamp/`. Its tests sit in `src/tests/`:
the path checks, the reader against the layout `wgsl_bindgen` generates, whole generations
(`a_second_build_reruns_nothing`,
`an_edited_shader_or_module_is_generated_again`, `a_shader_added_between_builds_is_generated`,
`a_crate_without_shaders_builds`), and the stamps in scratch repositories (`stamp.rs`), with
`a_commit_changes_a_watched_path_under_packed_refs_and_in_a_worktree` and
`the_engine_stamp_reads_cargo_vcs_info`, which runs `cargo package --no-verify` on the engine's
crates and henad-models, and compares henad-explore's crate hash and the source hashes of
henad-compute and henad-models with the checkout's. Keep them.
