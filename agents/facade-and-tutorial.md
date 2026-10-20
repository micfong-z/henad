# The facade and the tutorial

**henad**: the facade, the crate a program depends on. Its one `lib.rs` re-exports, and defines
nothing but the `params!` forward and its hidden `__macro_support`: the root items and macros,
`params`, `stats`, `views`, `action`, `gpu`, `runner`, `engine`, `explore`, `benchmark`
(native), `authoring` with its `primitives` and `prelude`, the
gated `models` (`example-models`), `app` (`app`), `cli` (`cli`), `testing` (`testing`) and
`rules` (`model-files`: the grid rules, the model files and `compose_wgsl`), and
`henad::prelude`. Each
re-export fixes its item's one documented path, and the guides name items by these paths alone.
`henad::explore` keeps henad-core's planning modules as modules (`spec`,
`plan`, `search` and the rest), and `henad::authoring` globs henad-core's `authoring::model`
modules flat, under `#![deny(ambiguous_glob_reexports)]`, so a name two of them share fails the
build. `binding` is the exception. The facade names its `BindingDecl` and `BindingKind`, as it
names each item of `helpers`, and leaves the engine's `buffer_target`, `RESERVED` and
`fmt_bytes` out. `params!` is a macro of the facade's own that forwards to henad-core's through
the hidden `__macro_support`. A `use` of henad-core's would bring its `params` module along, and
docs.rs would show that module at `henad::params`. The other root macros are plain re-exports.
henad-cli sits under a native target table, and `cli` adds nothing on wasm32. docs.rs builds the
facade for wasm32 too, without atomics, for the app's web entry point.
No feature changes a bound,
a layout or a result. `examples/complete.rs` is the program a newcomer reads first, and
`README.md`, which the crate doc includes with `app` and `example-models` both on, holds a copy in
a `rust,no_run` fence.
`the_readme_program_matches_the_example` (`lib.rs`) holds the two equal byte for byte, apart
from the example's `// --8<--` lines, which the README copy leaves out. The program prints with
`writeln!` to standard output and carries no lint attribute, so it copies into any crate. Its
`main` empties `sir-rates` in the temporary directory, hands `study` the example set, that folder
and standard output, then opens the app. `tests/complete.rs` includes the example whole, runs
`study` into a buffer on a folder named after the process id, removes that folder, and compares
the lines with the "It prints" block of `docs/guide/library.md`, where a `...` line stands for
any lines.
`tests/facade_paths.rs` compiles against facade paths alone, for the items the tutorial never
names. It defines no model, needs no build script and never builds on a device, and its parts
that need an entry compile in a module no test calls. That module also holds the `gpu_build`
and `scoped_steps` regions `guide/library.md` includes, the second through a `rayon`
dev-dependency. Its `named_only` module names items whose path is the whole check, the grid rule
items under `model-files` among them. Keep all three.

**henad-tutorial** (`examples/tutorial`, `publish = false`): the finished code of the five pages
under `docs/guide/first-model/`, which include each file whole. `life.rs`, `foraging/`,
`virus.rs`, `gpu_life/` and `gpu_foraging/` hold the models `life`, `foraging`, `virus`,
`gpu_life` and `gpu_foraging`, and `models()` (`lib.rs`) registers the five under the crate's
own `build_info!()`. The library and its tests name every item by a facade path, as a reader's
crate does: `henad::authoring::prelude::*` and `henad::authoring::...` in a model, the states
through `henad::engine`, the example models through `henad::models`. An item the tutorial needs
and the facade lacks goes into `crates/henad/src/lib.rs`, never into a direct dependency on an
inner crate. `build.rs` calls `stamp_commit` and `ShaderBuild::discover("src")`, and
`henad::include_shaders!()` brings the bindings in, as in a downstream crate. The two GPU
directories carry their own shader copies. `tests/shaders.rs` holds each equal to the shipped
one byte for byte, `gpu_foraging`'s apart from the import path `gpu_foraging::state`.
`tests/parity.rs` steps each tutorial model beside the example model it teaches and demands the
same bits (`the_game_of_life_tutorial_matches_the_shipped_model` and its siblings), and
`tests/kit.rs` runs `assert_set_conforms` over `models()` and joins the five to
`example_models()`. `tests/snippets.rs` holds every page block titled with a file under `src/` to
that file, line by line, apart from the first versions it lists, and the template's crate
attributes, with their comments, to those of `lib.rs`. `tests/thread_count.rs` holds
the foraging tutorial's thread-count test, outside `foraging/mod.rs`, which the ants page shows
whole and a reader's crate, without `rayon`, has to build. The modules in `lib.rs` are `pub`, and
a reader's crate declares them private, so a dead item shows only there. A change to a page is
checked by following it in a copy of the template, patched to the workspace by path. The
`package` job excludes it,
`scripts/check_packaging.sh` reads `crates/` alone, and `about.toml`'s `[private] ignore` keeps
it off the licence page. Keep the five test files.
