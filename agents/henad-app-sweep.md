# henad-app: the Sweep and Results tabs

`ui/sweep/` is the Sweep tab. `mod.rs` holds `SweepPanel` and `sweep_ui`, which splits the tab
into four egui panels: a header and a footer of fixed height (`header.rs`, `footer.rs`), a Plan
panel on the right while the tab is wide enough (`PLAN_PANEL_BREAKPOINT`), and a central scroll
area holding the form or the progress. The header and footer keep their ids and heights in every
state, and the last slot of the footer holds Start, then Pause or Resume, then Show results. A
press or a link sends a `SweepRequest`, applied once every panel has drawn. `dock.rs` turns off
`egui_dock`'s scroll bars for the tab and titles it with the session's state, as in "Sweep 42%".
`draft.rs` is pure and unit-tested: a `SweepDraft` per model holds the builder's fields as typed,
level text read by `LevelSpec::parse`, and converts to a `SweepSpec` (the Parameters panel values
fill every parameter it does not vary) and back, TOML through
`henad_explore::spec_file::SpecFile`. "One at a time" writes one factorial block per varied
parameter or action tick, each other varied parameter pinned to its panel value as a one-value
factor, since a spec cannot fix a parameter that any block varies. `from_spec` recognises that
shape and refuses other multi-block specs. `SweepDraft::check` collects every issue its pre-pass
finds before it plans, each a `DraftIssue` with the `DraftSite` of its row and an `IssueKind`:
`Missing` for input not given yet, `Invalid` for input the plan refuses. Both block Start.
`SweepPanel::cached_check` plans the draft only when it or the panel values change, and caps a
sweep at `MAX_DRAFT_RUNS`. The panel learns each model's stat columns from one build at its
default values (`ColumnsBuild` in `mod.rs`), on a thread named `henad-stat-columns` whose report
wakes the UI, a GPU model on a headless device of its own. A browser builds a CPU model inline
and skips a GPU model. `SweepDraft::set_stat_columns` hands the columns to the draft, which then
names outputs as `ReducerPlan::bind` does (a vector's `.x`, `.y` and `.magnitude`, a histogram's
`.total`), turns a watched bare vector label into its magnitude column, and in `check` binds the
reducers, the stop condition and a search's watched columns through `MeasurePlan::new` and
`SearchPlan::watched_reducers`. While the build runs, `columns_pending` holds the Missing issue
`COLUMNS_PENDING` ("Reading model outputs") at `DraftSite::Outputs` and Start stays disabled.
After a failed build each stat counts as one column, and a watched `Label.part:kind` of a
default kind is accepted for the sweep's own probe to check.
`row_levels` caps at `MAX_DRAFT_LEVELS` only a range the draft lists, and a range with no step
under a sampled design or a search can be any size. `SweepDraft::tick_source` (`TickSource`)
gives an action's tick as fixed, varied or taken from the design table's `action.<name>` column.
Save spec (`to_toml`) writes through `henad_explore::spec_file::LoadedSpec::to_toml`, with the
draft's execution table. It also refuses what the loader would, a reversed window or a threshold
that is not finite, and reads its own TOML back as a check. The draft keeps a loaded spec's `[execution]`
`memory` and `gpu_memory` (`memory_budget`, `gpu_memory_budget`). Save spec writes them back,
Start passes them to `SweepRunOptions` through `SessionExecution`, the Plan lists them, and the
Execution section shows them read-only beside a clear button. `MIN_TIMEOUT_SECONDS` bounds only
the Timeout field, and a loaded shorter timeout, 0 included, is kept, as `henad-cli` takes it.
`CheckSummary` turns the check into the lines the footer, its chips and the Plan panel list.
`plan.rs` (`PlanSummary`) is the plan the panel, the narrow Plan section
and a session draw. `layout.rs` holds `FormLayout`, the label column every section shares,
`SIDE_BY_SIDE_BREAKPOINT`, the section headers with their issue counts, the note line a row
reserves before any note comes, the `Reveal` that opens a section and scrolls to a row, and
`ISSUE_DELAY`, the time a focused text field holds back the issue of its text. `builder.rs`
draws the Design, Actions, Replicates and seeds, Run length, Outputs and Execution sections, and
`parameters.rs` the parameter rows, the values editor and the options menu, each writing the
row's `--vary` text, a list without spaces. `session.rs` wraps `henad_explore::handle::SweepRun`
(a thread on native, pumped from `HenadApp::logic` through `ui::sweep::update` in a browser, where
the frame keeps repainting while it steps), pauses the live sim on start, and keeps the
`KeptDraft` that started it for Save spec and the Plan panel. It hands `SweepRun` no device, so a
GPU sweep steps on a device of its own and never shares the app's `FaultSink`. A fault in
`render_ctx.faults` offloads the live model and leaves a running sweep alone, and `SessionState`
has no fault pause.
`progress.rs` draws the Status or Result grid, the Search grid and Runs in progress, and
`footer.rs` Pause, Resume, Abort with its modal, and the end with
Edit sweep, Save results and Show results. Show results sets `focus_request` to Results, as Open
on a run sets it to Viewport. Show failed runs and Show in Results go through
`ResultsPanel::show_failed_runs` and `ResultsPanel::select_candidate`. Events go to
`ui/results/`, the Results tab, whose `ResultsPanel` also keeps the in-memory files of the last
sweep for Save results. `store.rs` (`ResultsStore`) holds every run of the sweep, or of a folder
read through `henad_explore::result_set::ResultSet`, the configs with their axes (the value
columns that take more than one value), and a `SeriesCache` of series within
`DEFAULT_SERIES_BUDGET` bytes. It computes the Series view's replicate bands (`SeriesBand`), the
Response view's points and the heatmap's cells, each pooling a config's runs that did not fail
in order of run id, and the runs table's order. `summary.csv` pools in `runs.csv` order, which a
resume can leave out of run-id order, and the last digits can then differ. The other views cache
on `ResultsStore::revision`, and the Series view on the drawn configs' counts of runs and held
series plus `ResultsStore::replaced_count`, the runs a resumed sweep's rerun replaced.
`series.rs` draws a `FilledArea` band and a centre line per config, `response.rs` caches each
line as a `PlottedLine` and draws its whiskers as one `Whiskers` item with the line's id, so the
legend hides both, and `heatmap.rs` draws the grid. `plot.rs` holds the palette, `decimate` (the
lowest and highest point of each bucket), the heatmap scale and `HeatmapTiles`, the plot item the
Heatmap view and the PSE grid draw through. It borrows the values, and a cell without a value, or
an empty PSE cell, draws as `NO_DATA_COLOR`. `table.rs` draws the runs with
`egui_extras::TableBuilder`, and the detail strip whose Open and Open at end go through
`AppState::open_run`. Open at end steps to the run's recorded ticks, which a stop condition can
bring early. The store refuses to replay a run whose plan gives another run key than its row,
while the model's schema matches the sweep's. A folder whose manifest records more than
`MAX_PLANNED_CONFIGS` configs is never planned, lists its configs from `runs.csv` alone and
refuses every replay, on native and in a browser. After a model change the replay opens under the
table's warning, as it does when Henad's or the model's build differs from any a session recorded
(`ResultsStore::changed_builds`). A model the entry or the manifest declares does not replay
exactly gets a note beside Open (`replays_exactly`). Copy command puts an equivalent
`--export-stats` line of the product's `cli_command` on the clipboard, which samples from tick 0
on the CLI's own cadence. Open results reads a folder on a thread of its own on native and the
picked files in a browser, on the frame after the one that first shows "Reading results"
(`hold_picked_files`, `due_picked_files`).
`ui::results::poll` takes the `egui::Context` for it.
Resume sweep resumes an incomplete folder through `SweepRun::resume_directory`, with the memory
budgets its manifest records and automatic concurrency (`SessionExecution::recorded`), and the
folder is read again once that sweep ends. Resume is disabled while `ResultsStore::code_matches`
fails (`code_refusal`), for a folder of runtime or plugin code other than the app's entry
(`Manifest::code_matches`). `henad-app --open DIR` opens a folder at start.
`ui/sweep/search.rs` draws the Search section of Search mode: the method, the objective or the
PSE axes, the budget, and each method's other settings in a nested Method settings section. An
output picked there that the runs do not record joins the Outputs section, and Use range from
results reads `ResultsStore::output_range`. `SearchDraft` (`draft.rs`) keeps the settings of
every method across a switch. `ui/results/search.rs` is the Search view: the best so
far against evaluations for random search and hill climbing, the best, median and worst of each
generation for the genetic algorithm, and the PSE grid, where a click selects the first run of a
cell's exemplar. `ResultsPanel::ingest` takes a frame's events together, as `SweepSession::update`
drains them, and `ResultsStore::push_search_updates` records each `SearchBatchTold` batch by
batch, then adds the frame's new configs in one pass. The pass extends the configs and axes in
place (`fits_axes`, `extend_configs`), and a level keeps its id when levels are added below it.
Every config is built again only when a batch brings a second value to a column that is not an
axis, a text that is not a finite number to a numeric axis, or a candidate id at or below one
held. Runs that land before their candidate is told wait in `unassigned_runs`. A new level on a
numeric axis moves every level above it, and an `f32` search costs time linear in its levels per
frame.
