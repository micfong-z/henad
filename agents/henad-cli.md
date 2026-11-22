# henad-cli

**henad-cli**: headless benchmark runner, as a library with the official binary over it.
`lib.rs` holds the crate doc, `CliOptions` (a `ModelSet`, the host's `BuildInfo`, the command
name, the host's package name unless `command_name` sets another, and the `about` line help opens
with, henad-cli's description unless set), `SOME_RUNS_NOT_OK` and `run(options, arguments) -> u8`.
`run` installs the panic hook, parses with clap's `Command::name`, `bin_name` and `version` from
the options, and `about` when set, prints an error as `Error: {error:?}` and returns 1, and
returns clap's own code for `--help`, `--version` and a usage error. Argument parsing, the modes,
`Reporter`, `json_report` and the text formats are private. `lib.rs` also holds `Args`, `Mode`,
`run_args` with `headless_device` and `model_id`, and the `--info`, `--list` and `--params`
text. `model_file.rs` reads `--model-file`, `single.rs` runs one configuration (`run_single`, the
benchmark with its `Timings`, the two exports and `write_series`), and `explore/` runs sweeps.
`Mode::Single` carries a `SingleRun` (`ExportStats` and `ExportFinal` with their paths, and
`Benchmark`), which `run_single` matches with no catch-all, and every `match` on `Mode` in
`run_args` names each variant.
`json_report.rs` builds every `--json` line, one function per `kind`. `main.rs` is three lines
over `run` with `example_models()` and its `build_info!()`. henad-models is optional behind the default
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
`--export-stats` step a `Simulation` (`run_to` and `write_state`, `run_sampled`). For a model with
a time scale `export_stats` gives the `StatsWriter` the setup's tick length (`tick_length_in`),
which adds the `time` column. The shared
`params_by_id_json` and `scheduled_actions_json` sit in henad-explore's `output/details.rs`, for
the CLI's summary and the app's run details. A GPU run steps through `gpu/stepping.rs` on a device
from henad-explore's `acquire_headless`, sized to the `gpu_needs()` of the options' set with the
model files in it. A set without a GPU model asks for no device, apart from `--info`, and prints
no GPU note. The CLI resolves its positional id through that set's `ModelSet::lookup`, and
refuses an id outside the set ("this build does not include model 'x' (try --list)") apart from
a GPU model on a machine without a device. `--list` prints only the models this machine can run.
The CLI never publishes and never lays out a network.
`--model-file PATH` is repeatable, sits in `Args` outside `ExploreArgs`, and selects no mode.
`check_output_path` runs first for `--export` and `--export-stats`, and refuses an output path
whose canonical path is that of a model file (`an_export_onto_a_model_file_is_refused`). An output
that does not exist yet names no model file.
`with_model_files` runs after the pool is sized and `--merge` has returned, and before `--spec` is
read and the device acquired. It reads each file through `ModelFile::parse` and `entry` and
`ModelSet::replace`s its entry into a clone of the options' set, and the set is cloned only when a
file is given. `read_model_file` reads one file and prints its warnings, and `load_error` lists a
file's problems. A graph file's warnings print to stderr first, each through `located`, which adds
the file and a note naming the diagnostic's pass and node. A graph on a grid then builds through
henad-graph's `GridProgram`, and a graph of agents through its `AgentProgram`. Checker, host and
compiler errors come as `ModelFileError::Graph`. A sheet (`kind = "sheet"`) needs no code of its
own here: henad-explore places each of its warnings and errors at the sheet's line table and clears
the pass and node, so `located` prints the author line and no note. `with_model_files` returns
each `ModelFile` beside the set, and `run_single` takes the file of its entry: the device check
runs a sheet's `SheetFile::shortfalls`, so work past a budget names the sheet's lines and no pass
(`a_sheet_past_the_work_budget_is_refused_at_its_lines`). `explore::run` takes the file too, and
sets a sheet's `SheetFile::shortfalls` as `SweepOptions::shortfall_words`. The sweep's capacity
check then words each refused config in the sheet's lines
(`a_sweep_of_a_sheet_past_the_work_budget_is_refused_at_its_lines`), and so does the note of each
search candidate refused before its build
(`a_search_of_a_sheet_refuses_candidates_past_the_work_budget_at_its_lines`).
`--print-lowered` (`Mode::PrintLowered`, checked first) takes exactly one `--model-file`
(`parse_args` refuses another count with exit 2), requires it, conflicts with a model id, `--list`,
`--params`, `--info`, `--json`, the exports, `--stats-every`, `--merge`, every sweep flag and every
run flag (`--steps`, `--warmup`, `--global-warmup`, `--seed`, `--reps`, `--set`, `--act`,
`--threads`), and returns before the
model set is touched or a device acquired. It prints `ModelFile::lowered_graph`'s canonical text
on stdout (a graph file's own, a sheet's lowered graph), the warnings on stderr, and refuses a grid
rule with exit 1. `existing_invocations_keep_their_mode` expects `Mode::PrintLowered` of a
documented line holding the flag. A file whose id the set holds
prints a note on stderr in text mode and nothing under `--json`, and two files of one id are
refused. `run_args` prints a note in text mode for each file whose model is not the one the
command runs (`a_model_file_the_command_does_not_run_gets_a_note`). A file that does not load is an
error whose cause lists each `CodeDiagnostic` with the path as its file, `path:line:column: error:
message`, and exits 1 before any device. With one file and neither a positional id nor a spec, the
file's id is the model, under `--info` too (`Mode::of`). Two files or more without either are a
usage error from `parse_args`, exit 2. `--merge` conflicts with the flag. `tests/model_file.rs` runs
the binary on copies of henad-explore's `specs/models/life_rule.toml`, and holds
`a_model_file_sweeps_and_records_its_code`, `a_resume_with_an_edited_model_file_is_refused`,
`a_merge_of_shards_with_different_model_files_is_refused`,
`a_model_file_can_shadow_an_example_model`, `model_file_is_allowed_with_spec_and_resume`,
`list_and_params_json_show_a_model_file`, `a_model_file_error_points_at_its_line` and
`a_graph_model_file_checks_runs_and_prints_its_problems`, which reads henad-graph's Life starter
by path and sweeps it where a device is,
`a_sheet_model_file_runs_and_prints_its_problems_at_its_lines` (henad-graph's
`sheets/game_of_life.toml` by path, id `sheet_game_of_life`: 20 steps with no replace note and
`--list` holding both it and the example `game_of_life` where a device is, and a broken copy
refused at `[card.line.l2]` with neither "node" nor "pass" in the output, and a copy with no
property refused at `[world]` in the sheet's words, with no "lane" either) and
`print_lowered_prints_the_graph_a_sheet_lowers_to` (the printed text reads as a graph with the
sheet's code, a graph prints itself, a grid rule is refused, the flag's conflicts).
`a_model_file_error_points_at_its_line` and the two sheet tests run without a device, apart from the
sheet's run, and the others skip without one. Keep them.
A resume refused with `ResumeError::CodeChanged` for a model from a `--model-file` gets a first line
naming the file (`changed_file_context` in `explore/mod.rs`), and a merge refused with
`MergeError::CodeChanged` one naming the `--model-file` paths each shard's manifest records in
`argv` (`changed_files_text`). The library's messages stay as the causes below it.
`a_whole_number_below_zero_takes_whole_values_alone` and
`a_percentage_takes_a_percent_sign_in_levels` hold the integer mark and the `%` levels on sheets,
and `the_integer_key_marks_an_f32_and_refuses_anything_else` the `integer` key of a grid rule, of
the graph `--print-lowered` prints, and its refusals.
`--export` calls `prepare_view` before it writes, and `--export-stats` before each sample, as a
publish would.
`explore/` is the sweep mode: `mod.rs` runs a sweep or a merge, `args.rs` holds `ExploreArgs`,
its value parsers and `spec_from_flags`, and `report.rs` holds `Reporter` and the text of a plan,
its progress and its end. `--out`, `--spec` or `--dry-run` selects it (`Mode::Explore`), and
no command line from before sweeps changes meaning. `--list` refuses all three, and `--info`
beside them prints the provenance header and runs the sweep. It turns the flags in `ExploreArgs`
into a one-block `SweepSpec`, a factorial over every `--vary`, a zip, a `--sample lhs:N` or
`random:N` draw, or a `--design` table, or loads `--spec`. In a sweep, `--act` adds an action
named by its id, or by the first of `ID_2`, `ID_3` and so on that no earlier action took, and
`--vary action.NAME=LEVELS` varies its tick. `--spec` conflicts with every flag that changes a
result, and needs `--out`, `--dry-run` or `--params` (the `spec_use` group). A value parser
(`parse_set`, `check_act`, `parse_vary`, `parse_stop`, `parse_reduce`) refuses a malformed
`--set`, `--act`, `--vary`, `--stop` or `--reduce` before anything runs, with exit code 2, and
the model checks the ids later. Each but `check_act` returns the parsed value, which
`spec_from_flags` and `run_single` take as it is. `--act` stays text, since `Schedule::parse`
reads text. `--concurrent` (the lanes of a
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
`params_text` writes the bare word ` hidden` after `apply` and `format` for a hidden parameter
alone, and ` integer` after it for an `f32` marked integer, so no golden line moves.
`tests/model_file.rs` checks it, `time` and a `--vary` dry run over the tick length on
`sir_rates.toml`, read from henad-models' fixtures.
`tests/golden.rs` compares `--list`, `--params` and `--params --json` byte for byte with what
0.2.0 printed, recorded in `tests/golden/` by the procedure in its `README.md`. Keep them.
`build.rs` calls `henad_build::stamp_commit`, as henad-app's does, and the `CliOptions` host
build is the one the manifest records, the binary's `build_info!()` for `henad-cli`. The `info`
line's `engine_version` reads `ENGINE_BUILD.version()`. Under `--json` each `SweepWarning` also
prints as an `explore_warning` line (`json_report::explore_warning`), and so does the series
rows warning, as `series_rows`. `existing_invocations_keep_their_mode` reads the command lines of `lib.rs`, which
holds the crate doc, and of `docs/reference/cli.md`, and its floor is the 13 documented lines
that are not sweeps. `tests/json_lines.rs` runs the binary and holds each `--json` line kind to
the fields `benchmarks/protocol.md` and `docs/reference/cli.md` list, `explore_progress` aside,
which a quick sweep never sends and a unit test checks. Keep them.
The four test crates share `tests/support/mod.rs` through `mod support;`: `Scratch`, `run`,
`run_ok`, `stdout`, `stderr` and `manifest`.
