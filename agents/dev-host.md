# The dev host

henad-hot, henad-dylib and hot-host, and the two places `unsafe` code sits.

**henad-hot**: the dev host, native only. On wasm32 it holds only the plugin header and the two
macros. A project's app built with its `hot` feature under `[profile.hot]` links Henad dynamically
through henad-dylib, and its own library statically. `DevHost::rebuild` builds that library again
as a Rust `dylib`, the plugin. The plugin links the henad-dylib the app runs: one std, rayon, wgpu
and Henad serve both, a plugin panic unwinds into the app's `catching`, and a plugin GPU model
builds on the app's device. `header.rs` holds `PluginHeader` (`#[repr(C)]`: magic, ABI version,
henad-hot's version, the FNV-1a of `rustc -vV` its `build.rs` records, and an anchor address) and
`anchor()`, the address of a static. `lib.rs` holds `export_models!` (an `extern "C"` header
function, whose header is a `const` evaluated in the plugin, and a Rust-ABI models function, both
named with `_v1`), `dev_host!` (manifest directory, package and `HENAD_BUILD_FEATURES`) and
`__macro_support`. `cargo.rs` runs `cargo rustc --manifest-path .. --package .. --lib --crate-type
dylib --profile <profile> --no-default-features --features <host's> --message-format json --color
never` with `RUSTUP_AUTO_INSTALL=0`, the cargo `CARGO` names, else `cargo`, in the working
directory the process had when the dev host was created. It reads `compiler-artifact` and
`compiler-message` lines into `Artifact`s and `BuildDiagnostic`s (`BuildDiagnostic::from_json`
reads one line for a host), drains standard error on a thread of its own into a 40-line tail and
reports `HotProgress::WaitingForLock` once.
`package_dylib` picks the plugin among the artifacts, which cargo reports at their uplifted paths
(`target/hot/libhot_host.dylib`), and its tests read lines recorded from cargo 1.97. A
`BuildCancel` ends the build: the process group on Unix (rustix, `process_group(0)`), cargo's job
object on Windows. A cancel before cargo starts or before the load stops the call too, and the
next `warm_up`, `rebuild` or `check` clears it. `guard.rs` refuses before each build a `rustc -vV`
other than at the start or than the one that built henad-hot, and a changed Cargo configuration
file (`.cargo/config.toml` and `.cargo/config` up the tree from the working directory and in
Cargo's home, `GuardRefusal::CargoConfigChanged`). `RUSTFLAGS` in the environment stays fixed for
the process. After the build it refuses a henad-dylib cargo did not report fresh, one outside the
profile directory of the host's executable (its own directory, or the parent of a test's `deps` or
an example's `examples`) and that directory's `deps` (`HenadDylibElsewhere`, another profile,
target or target directory), one whose file (path, length and FNV-1a) differs from the warm-up's,
and at the warm-up one modified after the earlier of the executable's modification and the dev
host's creation. On Linux it also reads `/proc/self/maps` and refuses a henad-dylib that is not
the file the process maps (`HenadDylibChanged`, or `HenadDylibNotLoaded` for a host that runs
none), since glibc binds a plugin to a loaded library by soname. macOS and Windows show no such
mapping without `unsafe` code, and there the guard rests on `dev_host!` running first in `main`.
After the load it checks the header, the anchor included (`GuardRefusal::SecondImage`). `load.rs`
is the loader (see Lints). It reads the plugin's bytes, refuses a plugin whose symbol table
defines the allocator shim (`GlobalAllocator`, a defined symbol ending in `__rust_alloc`, as rustc
1.97 mangles it, or `__rg_alloc`, and `UnreadableLibrary` for a file the reader cannot parse),
writes the bytes into a fresh file and reads the copy back (`CopyDiffers` when its length or
FNV-1a differs, since `fs::copy` clones on APFS and evcxr saw a clone go stale), then opens the
copy, checks the header and calls the models function under `catching`. `symbols.rs` reads the
defined symbols of a 64-bit little-endian Mach-O (`LC_SYMTAB`, `N_SECT` entries), ELF (`.symtab`
and `.dynsym`, entries not `SHN_UNDEF`) or PE32+ file (export names), with no new dependency.
`dev_host.rs` holds `DevHost` (`new`, `profile`, `with_source_directories`, `package`,
`manifest_directory`, `source_directories`, `features`, `dependency_recompiled`, `cancel_handle`,
`warm_up`, `rebuild`, `check`) and `HotProgress`, whose `Loading` names the copy and the file
cargo built. `dependency_recompiled` reads whether cargo compiled a unit of a package other than
the plugin's and henad-dylib again (its artifact not `fresh`, `cargo::dependency_compiled`) since
the plugin the rebuild before returned, or since the warm-up, across refused loads. The package's
own library, which a save without a change compiles again, does not count. A load writes
the plugin to `henad-plugin-<package>-<pid>-<n>.<ext>` in a directory of its own,
`<profile directory>/henad-plugins/<package>-<pid>-<n>/`. The warm-up removes the directories of
ended processes (`remove_stale_loads`): a name that reads `<package>-<pid>-<n>` exactly, another
pid, and on Unix a pid that no longer runs. `rebuild` refuses an entry whose build records no
source hash, and returns every other entry marked as plugin code through
`ModelEntry::__into_plugin` (`plugin_models`). The source directories, `src` unless a host names
others (resolved against the manifest directory and normalised without the file system), change no
build, and a host lists their files for editing. The host and the plugin find henad-dylib and std
through the library path `cargo run` sets, and a dev host starts through `cargo run` alone. Its
tests sit in `src/tests/`, among them a scripted cargo on Unix
(`a_cancel_ends_the_build_and_its_children`, `a_cancel_before_the_build_starts_is_kept`), the
symbol reader on the test binary itself (`a_definition_is_found_and_a_reference_is_not`), and
`a_plugin_s_entries_read_as_plugin_code_of_their_crate`.

**henad-dylib** (`crates/henad-dylib`, outside the workspace with a `[workspace]` of its own,
published): `crate-type = ["dylib"]`, a `lib.rs` of `pub use henad;`, and `henad = "=X.Y.Z"` with
`hot`. Cargo builds a dylib with `-C prefer-dynamic` only as a non-primary package, so a host
and its plugins reach it as a dependency and share the toolchain's libstd. It inherits nothing,
and `check_packaging.sh` holds its package fields and requirement to the workspace's.
`scripts/check_hot.sh` lints it on a copy of the root lock.

**hot-host** (`examples/hot-host`, `publish = false`, outside the workspace with a `[workspace]`
and a lock of its own, the lock the root's pruned): Henad's own dev host and the test bed of
henad-hot. `models()` (`src/lib.rs`) returns `example_models()`, and under `check` the fixtures of
`src/tests/fixture.rs` after them: `fixture_grid`, whose kernel copies its cell, panics, spins
for good or writes `rayon::current_num_threads()` and whose live cells hold the generation, and
`fixture_gpu`, `GpuGameOfLife` with a step shader of its own, `src/tests/fixture_step.wgsl`,
which copies each word, and a `poison` parameter that shortens its uniform block. `build.rs`
calls `stamp_commit` and `ShaderBuild::discover("src")`, and `lib.rs` `include_shaders!`, as the
template's do. The generation comes
from `generation/` (package `hot-host-generation`, a member of hot-host's workspace), a
never-inlined function that host and plugin each link outside henad-dylib, as they link
henad-models. Under `hot` the library names henad-dylib (`use henad_dylib as _;`) and calls
`export_models!`. It takes the facade with `app` and `cli`, as a real dev host does, and
henad-models by path outside henad-dylib, so an edit to an example model reaches the next plugin.
`[profile.hot]` is `inherits = "release"`, the root's opt-level, `incremental` and `strip =
"none"`, with no `rpath`, equal to the template's (`check_packaging.sh`): Apple's linker refuses
to link against a stripped Rust dylib ("mis-aligned LINKEDIT string pool"), and a relative rpath
points one directory off for the binary cargo copies up. `src/main.rs` is the app binary
`hot-host`, the default run: the example models under "Henad Dev Host" with the official icon,
links and licence and `cli_command("hot-cli")`, and under `hot` `hot_reload(henad::dev_host!()?)`,
the dev host created first in `main`, with `crates/henad-models/src` among its source directories
(`with_source_directories`), so its Editor lists the example models' files. Its `inspection`
feature forwards to the facade's. `check_hot.sh` runs it with `HENAD_HOT_CHECK=1` and
`hot,check`, so a binary that links eframe and winit through henad-dylib links on arm64 macOS
(bevy#11779) and passes its self-check. `src/bin/hot-check.rs` runs as a `[[bin]]` with `hot` and
`check`, since a test binary resolves dev-dependency features, and `check_hot.sh` starts it twice
through `cargo run`.
The first run checks, in order, `a_plugin_build_uses_the_host_features`,
`a_plugin_build_from_the_running_host_keeps_henad_dylib_fresh`,
`the_plugin_binds_to_the_host_image`, `every_plugin_entry_reads_as_plugin_code_of_its_sources`
(plugin origin, and the package, type path, declarations and source hash of the host's static
copy), with the first load not compiled again,
`every_example_model_from_the_plugin_steps_like_the_static_one` (50 ticks, stats every ten and a
CPU model's state, ticks and population alone for a model that does not replay exactly),
`a_plugin_panic_is_a_located_fault`, `a_plugin_pass_honours_the_lane_pool_width`,
`a_plugin_gpu_model_builds_on_the_host_device`, `the_kit_passes_on_plugin_entries`,
`a_plugin_panic_on_a_runner_is_a_located_fault` (the fault of a `SimThread::new_in` run, at its
line, and the runner joined), `a_plugin_with_a_global_allocator_is_refused` (a
`#[global_allocator]` appended to `generation/src/lib.rs`, refused before any load),
`a_failed_build_is_reported_at_its_line` (a type error in `generation/src/lib.rs`, then a load),
`an_edit_to_a_cpu_step_cell_reaches_the_next_load` (`fixture_grid`'s `step_cell` edited to add
one, a new source hash, then a type error at its line with the earlier load stepping on, then the
restored source), `an_edit_to_a_wgsl_step_shader_reaches_the_next_load` (with a device: the copy
edited to clear every word under a new source hash, then a missing semicolon placed by the `┌─`
line of the build script's standard error with the earlier load stepping on, then the restored
shader), `an_edit_outside_henad_dylib_reaches_the_next_load` (20 edits of the generation,
restored after, each read from the plugin's copy and not the host's, each compiled again under
the fixture's unchanged source hash, each copy in a directory of its own and hashing as cargo's
output, with the resident memory printed per load) and, last,
`a_plugin_step_that_never_returns_is_detached` (a spinning `step_cell` on a runner in a pool of
its own, detached within `DEFAULT_STOP_TIMEOUT`, and a run on a fresh pool stepping while it
spins until the process ends). Each edit is restored when its check ends, failed or not. The
second,
`hot-check relink`, loads a plugin, sets the modification time of `crates/henad-dylib/src/lib.rs`
to now and runs `a_changed_henad_build_is_refused_before_loading`: the build that rebuilds
henad-dylib under the running host is refused before any load (`HenadDylibRebuilt`, or on Windows
a failed link naming henad-dylib), the loaded plugin and a linked model step on, and the next
build is refused again or, on macOS where the relink can write the same bytes, loads. It runs
last, and the next `cargo run` relinks the binary. Keep them.

## Unsafe code

Beside the generated shader bindings, `unsafe` code sits in two places, and nowhere else.
henad-hot's `load.rs`, the dev host's loader, is declared under `#[expect(unsafe_code, reason =
..)]` and holds exactly three `unsafe` blocks: `Library::new`, the `extern "C"` header symbol and
the Rust-ABI models symbol. Each carries a `// SAFETY:` note naming the guard that ran before it,
and a loaded library is leaked with `Box::leak`, never unloaded (`mem_forget` is linted). The other
is the expansion of `export_models!` in a model crate: two `#[unsafe(no_mangle)]` functions,
`__henad_plugin_header_v1` and `__henad_plugin_models_v1`, each under an item-level
`#[allow(unsafe_code)]`, as `include_shaders!` allows the lint over the bindings. Their names end in
`PluginHeader::ABI`, and a `const` assertion in `load.rs` holds the two to it. A crate under
`#![forbid(unsafe_code)]` rejects both functions.
