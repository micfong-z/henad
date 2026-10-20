# henad-cli

**henad-cli**: headless benchmark runner, as a library with the official binary over it.
`lib.rs` holds the crate doc, `CliOptions` (a `ModelSet`, the host's `BuildInfo`, the command
name, the host's package name unless `command_name` sets another, and the `about` line help opens
with, henad-cli's description unless set), `SOME_RUNS_NOT_OK` and `run(options, arguments) -> u8`.
`run` installs the panic hook, parses with clap's `Command::name`, `bin_name` and `version` from
the options, and `about` when set, prints an error as `Error: {error:?}` and returns 1, and
returns clap's own code for `--help`, `--version` and a usage error. Argument parsing, the modes,
`Reporter`, `json_report` and the text formats are private. `main.rs` is three lines over `run`
with `example_models()` and its `build_info!()`. henad-models is optional behind the default
`example-models` feature, which the `[[bin]]` and the four integration tests that run it name in
`required-features`, and `cargo build -p henad-cli --no-default-features` builds the library
alone. Steps a state in a bare loop with no rendering, no
`SimThread` and no pacing, so a measurement times nothing but `step()`. `--act ID@TICK`
(`henad_core::action::Schedule`) runs a declared action before the step at that tick. An action
due from the end of warm-up up to, but not including, the tick the run stops on is timed with the
steps, and one due on that tick runs after the timer stops. The GPU path fires the same ticks.
The GPU benchmark fires before the step and the GPU stats export after it, one `action::Fire`
rule per run, as the two CPU loops do. Otherwise two runs back to back both fire the tick they
share. The benchmark is `henad_explore::benchmark::run_benchmark` (`benchmark.rs`), which holds
the rule, `BENCH_FIRE`, and reports each repetition through a callback as it starts and as it
finishes, so a run killed part way has printed every repetition it finished. `--export` and
`--export-stats` step a `Simulation` (`run_to` and `write_state`, `run_sampled`). The shared
`params_by_id_json` and `scheduled_actions_json` sit in henad-explore's `output/details.rs`, for
the CLI's summary and the app's run details. A GPU run steps through `gpu/stepping.rs` on a device
from henad-explore's `acquire_headless`, sized to the `gpu_needs()` of the options' set with the
model files in it. A set without a GPU model asks for no device, apart from `--info`, and prints
no GPU note. The CLI resolves its positional id through that set's `ModelSet::lookup`, and
refuses an id outside the set ("this build does not include model 'x' (try --list)") apart from
a GPU model on a machine without a device. `--list` prints only the models this machine can run.
The CLI never publishes and never lays out a network.
`--model-file PATH` is repeatable, sits in `Args` outside `ExploreArgs`, and selects no mode.
`with_model_files` runs after the pool is sized and `--merge` has returned, and before `--spec`
is read and the device acquired. It reads each file through `ModelFile::parse` and `entry` and
`ModelSet::replace`s its entry into a clone of the options' set, and the set is cloned only when
a file is given. A file whose id the set holds prints a note on stderr in text mode and nothing
under `--json`, and two files of one id are refused. A file that does not load is an error whose
cause lists each `CodeDiagnostic` with the path as its file, `path:line:column: error: message`,
and exits 1 before any device. With one file and neither a positional id nor a spec, the file's
id is the model, under `--info` too (`Mode::of`). Two files or more without either are a usage
error from `parse_args`, exit 2. `--merge` conflicts with the flag. `tests/model_file.rs` runs
the binary on copies of henad-explore's `specs/models/life_rule.toml`, and holds
`a_model_file_sweeps_and_records_its_code`, `a_resume_with_an_edited_model_file_is_refused`,
`a_merge_of_shards_with_different_model_files_is_refused`,
`a_model_file_can_shadow_an_example_model`, `model_file_is_allowed_with_spec_and_resume`,
`list_and_params_json_show_a_model_file` and `a_model_file_error_points_at_its_line`. The last
needs no device, and the others skip without one. Keep them.
`--export` calls `prepare_view` before it writes, and `--export-stats` before each sample, as a
publish would.
`explore.rs` is the sweep mode. `--out`, `--spec` or `--dry-run` selects it (`Mode::Explore`), and
no command line from before sweeps changes meaning. `--list` refuses all three, and `--info`
beside them prints the provenance header and runs the sweep. It turns the flags in `ExploreArgs`
into a one-block `SweepSpec`, a factorial over every `--vary`, a zip, a `--sample lhs:N` or
`random:N` draw, or a `--design` table, or loads `--spec`. In a sweep, `--act` adds an action
named by its id, or by the first of `ID_2`, `ID_3` and so on that no earlier action took, and
`--vary action.NAME=LEVELS` varies its tick. `--spec` conflicts with every flag that changes a
result, and needs `--out`, `--dry-run` or `--params` (the `spec_use` group). A value parser
(`check_set`, `check_act`, `check_vary`, `check_stop`, `check_reduce`) refuses a malformed
`--set`, `--act`, `--vary`, `--stop` or `--reduce` before anything runs, with exit code 2, and
the model checks the ids later. `--concurrent` (the lanes of a
CPU model or the tracks of a GPU model), `--memory`, `--gpu-memory`, `--shard`, `--resume` and
`--retry-failed` pass through to `SweepOptions`, and `--merge DIR... --out DIR` (`Mode::Merge`)
calls `merge` with no model and no device. It renders `ProgressEvent`s as a text line on stderr
or as `explore_*` JSON lines, and exits 3 (`SOME_RUNS_NOT_OK`) when a sweep ran to its end with a
run not `ok`, or a merge lacks a run. A sweep seeds its runs with `run_seed`, and the benchmark
keeps the `base + i` that `benchmarks/protocol.md` fixes.
It applies a spec's `[execution]` table, then the flags it was given, and calls `run_spec`, or
`plan_spec` for `--dry-run`. A spec with a `[search]` table runs as a search. Its plan lists the
batch size, the objective or the axes, the space and the search seed, and `--json` adds an
`explore_search_batch` line per batch.
`--params --json` (`json_report::params`) prints `schema_json` with a `kind` of `params`.
`scripts/bench_matrix.py` parses the text `--params` prints, and that text is unchanged.
`tests/golden.rs` compares `--list`, `--params` and `--params --json` byte for byte with what
0.2.0 printed, recorded in `tests/golden/` by the procedure in its `README.md`. Keep them.
`build.rs` calls `henad_build::stamp_commit`, as henad-app's does, and the `CliOptions` host
build is the one the manifest records, the binary's `build_info!()` for `henad-cli`. The `info`
line's `engine_version` reads `ENGINE_BUILD.version()`. Under `--json` each `SweepWarning` also
prints as an `explore_warning` line (`warning_json`), and so does the series rows warning, as
`series_rows`. `existing_invocations_keep_their_mode` reads the command lines of `lib.rs`, which
holds the crate doc, and of `docs/reference/cli.md`, and its floor is the 13 documented lines
that are not sweeps. `tests/json_lines.rs` runs the binary and holds each `--json` line kind to
the fields `benchmarks/protocol.md` and `docs/reference/cli.md` list, `explore_progress` aside,
which a quick sweep never sends and a unit test checks. Keep them.
