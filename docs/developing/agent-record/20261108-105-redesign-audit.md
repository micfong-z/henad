---
date: 2026-11-08
title: "Redesign audit"
description: "S32 of the #37 redesign. Eight review lenses with fresh contexts, each finding checked by a verifier, found 48 confirmed faults and 14 rejected ones over records #71 to #104. Fifteen fix commits change the lowering's order rules, the identity tests, start keys, the grid lockstep, the probed barriers, small rates, Compare runs, GPU pacing, Run to tick sampling, the sheet words, packaging, comments, topic files and docs. The full check matches the baseline, and all ten end-to-end items pass, two after fixes."
icon: material/clipboard-check-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#37"
state: "37-node-editor, fast-forwarded to redesign/s32-audit, based on 81278fc: fifteen unsigned [draft] fix commits and this record; the full check with HENAD_REQUIRE_GPU=1 passes every step but cargo deny and the workspace tests, which fail as the baseline does, on its three advisories and its one budget test"
baseline_commit: 81278fc
delta_state: "committed on `37-node-editor` by fast-forward: fifteen `fix: redesign audit ... [draft]` commits and this record, unsigned, for the maintainer to reword and sign"
---

# Redesign audit

> S32 audits everything the redesign added, records #71 to #104, through eight lenses: semantics, identity, determinism, GPU, UI, style, dependencies and a researcher's walk through the app.
> Each lens ran as a finder with a fresh context and the milestone records, and asked for findings outside them, and a verifier checked each finding before any fix.
> Of 62 findings, 48 were confirmed (4 must fix, 21 should fix, 23 nits) and 14 rejected with a reason.
> Fifteen `fix:` commits address 47 of the 48 in full or in part, most with a test that fails without the fix, and the parts left open are listed below with a reason.
> The largest fixes: a sheet now plays at a speed a person can watch, Run to tick charts the ticks between its start and end, Compare runs no longer names a difference for a model that does not replay exactly, and a rate below the 24-bit resolution of a float draw keeps its chance.
> The full check matches the baseline, `docs_rs.py` and `zensical build` pass, and all ten end-to-end items pass, two of them after the fixes they found.
> The screen was locked throughout, so the native app was checked through its accessibility tree, and only the browser item has screenshots.

## State before

`37-node-editor` at `81278fc` held S0 to S31, records #71 to #104, as 131 unsigned `[draft]` commits since `f4088d7`.
Each milestone had passed its own review and the full check at its merge, and two baseline failures remained: `cargo deny` on RUSTSEC-2026-0370, 0371 and 0372, and the workspace test `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets`.
No review had looked at the redesign as a whole, run the end-to-end list of plan S32, or run `python3 scripts/docs_rs.py` since record #66.

## What was done

### The lenses

Each lens was a finder with a fresh context, given the records of #71 to #104 and asked for findings that no record already lists.
A verifier with its own context then checked each finding against the code and the design documents, and marked it confirmed or rejected.
The eight lenses:

1. **Semantics.** The lowering against round 1 section 6 (reads see the start of the tick, changes land at its end, the higher change wins, one Become per tick), "otherwise" and "or", rates and competing groups.
2. **Identity and provenance.** Every row of the author file's identity table, the manifest's sheet text, resume and merge.
3. **Determinism, keys and replay.** Chunk seeds, composed keys, presses and starts that draw.
4. **GPU and performance.** `agents/gpu-traps.md`, uniform control flow, bindings, workgroup memory, fast math, the fire-count channel and `AGENTS.md`'s invariants.
5. **UI.** The words rule, accessible names, colours, corners, tense and tooltips per `agents/ui.md`.
6. **Code and docs style.** `AGENTS.md`'s comment rules, missing docs, `Debug`, semantic line breaks and includes by region.
7. **Dependencies and packaging.** `agents/dependencies.md`, `check_packaging.sh`, `cargo deny` and the docs.rs build.
8. **Researcher walk-through.** A simulated researcher with no access to the code rebuilt Game of Life and SIR with rates from Blank in the real app through the egui MCP server, and wrote down every stall.

### Confirmed findings and their outcome

The commits are named by their short hash on `37-node-editor`.
A test named in an outcome fails without its fix unless the outcome says otherwise.

#### Semantics

| Id | Severity | Finding | Outcome |
| --- | --- | --- | --- |
| F1 | must fix | A result chip was treated as one value per element, so two changes inside a Repeat that test a Random number drawn inside it were judged unable to both apply. The card showed no "otherwise", and the transition rows fell back to hidden line id order. | Fixed in `c99d870`. `CardRelations::compare_test` returns no test for a result whose line sits inside a Repeat (the new `inside_repeat`, which `group_key` shares), so the lower line reads "otherwise" and the rows keep display order. Test `becomes_that_test_a_number_drawn_inside_a_repeat_keep_display_order`. |
| F2 | should fix | Record #92 item 7 said the departure from round 1's "the first time it applies" shows only with carried values. Two plain chance lines inside a Repeat already apply at different times round, and no decision row records the any-time rule. | Docs fixed in `c99d870`: record #92 item 7, `docs/reference/model-files.md` and `agents/henad-graph-sheet-lower.md` now say the higher line wins whichever time round each held. The rule is unchanged and goes to the maintainer (decisions below). |
| F4 | nit | A Keep between displayed above a Set clamped that Set's value, against the `motion.rs` module doc. | Fixed in `c99d870` by matching the doc and the topic file to the lowering: a Keep between holds the value after every Set of the property and after the Steers above it. Host test `a_keep_between_holds_the_value_after_every_set`, both orders. |
| F5 | nit | Each Move to joined its open test into its condition, so a lower Move to moved the agent when a higher one held but its place was not open. | Fixed in `c99d870`. `write_moves` ends the chain at the first Move to whose condition holds, and a blocked place keeps the agent where it is. A single Move to lowers as before, so the ants' code is unchanged. Test `the_first_move_to_whose_condition_holds_decides` on `src/tests/sheets/move_to_first.toml`. The "otherwise" words for the lower Move to are not added (below). |

#### Identity and provenance

| Id | Severity | Finding | Outcome |
| --- | --- | --- | --- |
| L2-F1 | should fix | Reordering two Steers on one property changed the lowered graph and the code, so a sweep resume was refused, against the author file's identity table and `docs/reference/model-files.md`. | Fixed in `5653500`. `steered` adds the Steers between two Keep betweens in line id order. Test `reordering_steers_of_one_property_keeps_the_lowered_graph`. |
| L2-F3 | should fix | Eight rows of the identity table had no test, the `rate_unit_from_tick` mark's place outside the code had none, and no sheet test asserted a schema hash. | Fixed in `5653500`: one test per missing row in `sheet_lower.rs`, through a `changes()` helper that asserts the code and the schema hash, and the existing identity tests now assert the schema hash too. |
| L2-F4 | nit | The table's schema cell for a change of the time scale said "same", but a tick length change alters the hidden `tick_length` parameter and so the schema hash. | A test in `5653500` pins it: the length changes the schema hash, the unit alone keeps it. The design table is corrected outside the repository. |
| L2-F5 | nit | `model-files.md` said a sheet's ids enter the code, and a measure's id does not. | Fixed in `5653500`. The reference names the ids that enter the code and lists a measure's id, a parameter's unit and its means outside it. Test `changing_a_measure_id_keeps_the_code_and_the_schema`. |

#### Determinism, keys and replay

| Id | Severity | Finding | Outcome |
| --- | --- | --- | --- |
| D1 | nit | A press's draws depend on its button's place among the sheet's buttons, so adding a button above another changes that button's numbers while its lines keep their keys. | Documented in `5653500` in `model-files.md` and the lowering's topic file. Keying the phase on the action id is left to the maintainer, since it would change the action draws of every graph model. |
| D2 | should fix | Compare runs compared measures exactly without checking whether the model replays exactly, so an unedited conversion of bounded confidence or boids reported a difference. | Fixed in `3757ca5`. `CompareOutcome::Inexact` with the sentence `runs_do_not_replay` when either entry's `replays_exactly` is false, and the guide and topic file corrected. Test `compare_runs_of_a_model_that_does_not_replay_exactly_names_no_first_difference`. Seen in the app's accessibility tree under the lock. |
| D3 | should fix | An agent start on the ground with its state painted on two or more cells draws, but neither the checker nor the editor counted it as drawing, so it never took a key and its numbers followed the sheet id. | Fixed in `e9bf78c`. The checker warns "draws and holds no key" and the editor's `starts_draws` mints a key. Test `a_start_on_two_painted_cells_takes_a_key`. |
| D4 | nit | Host against device lane equality covered five grid starters only: no SIRD competing draw, no press that draws, no agent sheet. | The grid half fixed in `b9f9660`: `every_grid_sheet_steps_alike_on_the_host_and_the_device` takes the SIRD sheet and presses New outbreak on host and device after tick 10. The agent half is not built (below). |

#### GPU and performance

| Id | Severity | Finding | Outcome |
| --- | --- | --- | --- |
| G1 | should fix | A chance below about 1e-6 per tick ran at the wrong probability on 24 bits, and a rate below about 3e-8 per tick never fired, while the card's note showed the intended chance. | The rate path fixed in `86dd774`. A rate the same for every element, with no "for each", takes its chance from a series below a hazard of 2^-10 and draws through a 32-bit threshold. Tests `a_small_rate_keeps_its_chance_in_the_threshold` (host) and `a_rate_below_the_resolution_of_a_float_draw_recovers_its_share` (device, SIR at 2048 by 2048, about 50 recoveries where there were none). The SIR starter's code changes with it. The other paths stay (below). |
| G2 | should fix | No test held the barriers of the probed shaders in uniform control flow, though every sheet compiles with probes since S31. | Fixed in `6c90d49`. `every_probed_shader_keeps_its_barriers_in_uniform_control_flow` over the probed graph starters, every sheet starter and two grid fixtures. |

#### UI

| Id | Severity | Finding | Outcome |
| --- | --- | --- | --- |
| UI-1 | should fix | A name that maps to a reserved id (Count, Grid width, Tick length) showed the library's raw message with the word "id". | Fixed in `1477555`. The forms refuse it first with `sentence::name_reserved` ("Henad uses the name Count itself. Choose another name."), and `widgets::refusal_text` never shows the id. Seen in the app. |
| UI-2 | should fix | Cards over agents or the ground reused sentences that say "cells". | Fixed in `1477555`. Those sentences take the card's thing word, with new Lines and World help for agents. Test `the_notes_of_an_agent_card_name_its_agents`. |
| UI-3 | should fix | In sheet mode, problems not drawn under a line were listed as "line:column message", a position in a text the author never sees. | Fixed in `1477555` with W4. |
| UI-4 | should fix | The words-rule tests matched whole words only, so plurals got through, and the lowering's limit messages held "nodes", "lowers" and a line id. | Fixed in `1477555`. `sentence::is_forbidden_word` catches the usual endings, and the limit messages are new sentences with neither. |
| UI-5 | nit | The work-budget messages ended in "... would fit." in place of an imperative. | Fixed in `1477555`, as "Use fewer birds." and the like. Record #87's quotes stay as written then. |
| UI-6 | nit | Apply outcome and preview lines are app literals with words outside the wording table. | In part, in `1477555`: an unchanged Apply reads "Model unchanged.". The verifier found the rest free of forbidden words. |
| UI-7 | nit | The New model tooltips said "from an example" though the dialog opens on Blank. | The tooltips fixed in `1477555`. Blank's description stays as gate section 10.13 words it. |

#### Code and docs style

| Id | Severity | Finding | Outcome |
| --- | --- | --- | --- |
| F1 | should fix | Source comments cited decision ids and unpublished plan and study documents. | Fixed in `87df8a9` across 16 files. |
| F2 | should fix | Topic files carried about 147 plan and decision ids. | Fixed in `d19871c`. Ids from before the redesign stay. |
| F3 | should fix | A stale paragraph in `agents/henad-explore.md` described an interim rate refusal and a later milestone. | Fixed in `d19871c`. |
| F4 | nit | Comment lines added by the redesign ran past 120 columns. | Fixed in `87df8a9` for every such line `git blame` dates after `f4088d7`. The older ones stay (below). |
| F5 | should fix | About 58 new doc comments opened with a headless "What/How/Where" clause. | Fixed in `87df8a9`: 59 rewritten. The 37 from before the redesign stay. |
| F6 | should fix | Migrating to 0.4 had no step for the output writers' new arguments. | Fixed in `ed63ffb` with deps-2. |
| F7 | should fix | The 0.4 CHANGELOG lines were long and used study jargon. | Fixed in `ed63ffb`: the redesign's lines cut to the names a user meets, the longest from 1998 characters to 640. |
| F8 | nit | Five list items in the editor guide held two sentences on one line. | Fixed in `ed63ffb`. |
| F9 | should fix | `docs/guide/sweeps.md` had two sections on the tick length. | Fixed in `ed63ffb`: one section, "Varying the tick length". |
| F10 | nit | The landing page explained only the #48 records' unpublished citations. | Fixed in `ed63ffb`: `agent-record.md` explains the plan, gate and study ids of records #71 onward. |
| F11 | nit | Some topic-file lines broke the hard wrap, and `agents/henad-graph-sheet.md` sat close to 40k. | Fixed in `d19871c`. The file is 39,728 bytes. |
| F12 | nit | New identifiers used British spelling. | Fixed in `87df8a9`: `lower/colours.rs` is `colors.rs`, with `neighbor_count`, `uncolored`, `center_y` and others. Two pass names and one public variant stay (below). |
| F13 | nit | `ChangeKind::Become` named its fields `from` and `to`. | Fixed in `87df8a9`: `old_state` and `new_state`. |
| F14 | nit | Records break the one-sentence-per-line rule and order their extra sections two ways. | No record was changed. This record keeps one sentence per line and the heading "Benchmark commands left for the maintainer". |

#### Dependencies and packaging

| Id | Severity | Finding | Outcome |
| --- | --- | --- | --- |
| deps-1 | must fix | The docs.rs build of henad-explore failed on wasm32, since `SweepRun`'s doc linked the native-only `stop`. The line predates the redesign, and CI's docs job would have failed. | Fixed in `217fb5f`. `scripts/docs_rs.py --host` builds henad-explore, henad-graph and the facade on aarch64 and wasm32. |
| deps-2 | should fix | Migrating to 0.4 lacked the output writers' new arguments. | Fixed in `ed63ffb`. |
| deps-3 | nit | The `ManifestModel` migration step named `author` but not `code`. | Fixed in `ed63ffb`. |
| deps-4 | nit | The crates.io and docs.rs texts described henad-graph as graph models only. | Fixed in `217fb5f`: the Cargo description, READMEs, crate doc, facade feature comment and `AGENTS.md` name sheets. |
| deps-5 | nit | `agents/dependencies.md` gave one test module as the reason for henad-models' dev edge to henad-graph. | Fixed in `217fb5f`. |
| deps-8 | nit | The facade's `henad::graph` named every public module of henad-graph but `normal`. | Fixed in `217fb5f`, with `henad::graph::normal::Origin` in `facade_paths.rs`. |

#### Researcher walk-through

| Id | Severity | Finding | Outcome |
| --- | --- | --- | --- |
| W1 | must fix | A sheet, which always runs on the GPU, could not play at a watchable speed: Pacing had no TPS cap for GPU models, and SIR on 100 by 100 ran 23,474 days in about 3 s. | Fixed in `9eab8e5`. The GPU runner keeps to `SetTargetTps` once `SetUncapped(false)` arrives, and the Pacing tab shows Unlimited TPS and Target TPS for GPU models and sheets. Test `a_capped_gpu_run_keeps_to_its_rate`. In the app, capped Play ran about 38 ticks in 4 s. |
| W2 | must fix | On a GPU model, Run to tick charted only its start and end. | Fixed in `9eab8e5`. `Snapshot::run_to_samples` holds stats at up to 500 evenly spaced ticks, on the CPU and on native GPU, carried over a publish the app missed. Tests `a_run_to_a_tick_samples_its_stats_on_the_way` and `a_gpu_run_to_a_tick_samples_its_stats_on_the_way`. |
| W3 | should fix | A second new model got no preview and gave no action to take, and its Apply needed Build. | In part, in `1477555`: the waiting preview reads "Preview waits for the running model. Press Offload in Playback to show this one.". The rest was decided in records #55 and #65. |
| W4 | should fix | The sheet pane showed "21:1 a value is blank, where it wants a chance". | Fixed in `1477555`. Sheet mode lists the message alone, and the checker's error is `sentence::empty_field` ("Health has an empty field. Type a chance in it."). Test `an_empty_start_chance_names_its_property`. |
| W8 | nit | The Charts legend was alphabetical. | Fixed in `1477555`: the legend follows the measures' order. |
| W9 | nit | Play and Step were named only by their icon glyphs in the accessibility tree. | Fixed in `1477555`, and seen in the tree. |
| W10 | nit | A state's Plot box wrapped away from its name. | Fixed in `1477555`, then again in `b1a3106` (end to end item 3). |

### Rejected findings

| Id | Finding | Reason |
| --- | --- | --- |
| F3 (semantics) | A colliding line between two lines of one competing group keeps the lower group line in the group. | Decided in records #91 and #100 ("'or' yields to 'otherwise'"), and the run matches what the card says. Only the edge case is named here. |
| L2-F2 | Show changes guards on the hash of the canonical text, not the code. | The expert design defines `origin.hash` that way, and plan S23 repeats it. Loosening it is the maintainer's call. |
| L2-F6 | `origin.hash` and the manifest's author hash use different hashes. | Each follows its own documented convention, and the documented link, `origin.code` against `model.code.fnv1a64`, uses one hash. |
| D5 | On the web, minted keys may repeat across sessions. | Predates the redesign, and every minted key is checked against the file's keys. A seed from `web_time::SystemTime` stays an option. |
| G3 | Probes might change the float results of the step shader. | Probes add only the tally lines, the other shaders are byte-identical, and record #66 found one device agrees with itself on one float expression. |
| G4 | A per-element group's division differs from the host under fast math. | Falls under the accepted engine-wide fast-math condition, the shipped fixed-rate case divides on the host, and the effect is at most about 2^-24 per element and tick. |
| UI-8 | Display names spell "color" while sheet words spell "colour". | Each follows a written rule, and record #81 already leaves the question to the maintainer. |
| UI-9 | The Diagram is one image node in the accessibility tree. | Gate section 10.18 specifies only its name, and the card's lines already name every result. |
| deps-6 | Tests read files across crates, and henad-app holds two copies of a fixture. | An established pattern since before the redesign, no job runs tests from a tarball, and record #91 documents the copies. |
| deps-7 | The engine stamp leaves out the starter sheets. | A starter is model input, and a run records the sheet's own code hash. Widening the stamp is a henad-build design change. |
| W5 | The Model tab shows the selection, not the Editor's preview. | Record #86 builds Build's behaviour on that split. |
| W6 | Playback lacks a Speed control in sheet words. | D60 exempts the world tabs' controls and D86 leaves the gate's Speed row unbuilt. W1's fix restores the TPS controls. |
| W7 | "One tick is 1 days" reads oddly. | The gate's Time row gives the select those items. |
| W11 | The Charts axis shows few labels at the default width. | Already listed in record #79, and its cause is egui_plot's grid spacing, not W2. |

### End to end

The full check ran in three foreground chunks at `ed63ffb` and again at `802a4b2`, after the two end-to-end fixes.
Then `python3 scripts/docs_rs.py --host` and `uv run zensical build --clean`, which reports no issues and leaves no `--8<--` directive unresolved outside the records.

| Item | Result |
| --- | --- |
| 1. The CLI runs the Game of Life sheet, and a broken copy prints the author line | Pass. 100 steps at 1024 by 1024. Two broken copies print `path:95:1` "the state alive has no property to belong to" and `path:83:1` "colour is no property of cells", with no node or pass. |
| 2. Game of Life from Blank with a random start, Step and Play | Pass. Built through accessible names: Status, By chance 30%, Plot Alive and three lines. Step advanced 0, 1, 2, and Alive fell from about 3000 to 291 and held there from tick 12,156 to 25,170. The blinker and glider known answers pass in the full check, as D74 asks. |
| 3. SIR with rates from Blank | Pass after `b1a3106`. The notes read the chances 0.259 and 0.451, "Up to 0.909 with 8" and a recovery chance of 0.0952. Playback reads "Tick 22 (22 days)", Apply reads "Model applied.", and Run to tick reaches "Tick 80 (80 days)". The browser screenshot shows the Day axis. |
| 4. The SIRD fixture's competing lines read "or", and its Dead share at a 7-day tick stays near one half | Pass. The added line reads "or become Dead from Infected, at rate 0.1 per day". A CLI sweep at `tick_length` 7 gives a Dead share of 0.5002 to 0.5007 over ticks 5 to 40 on 3 seeds. |
| 5. Boids and ants open, run and take an edit | Pass. Separation 0.05 to 0.1 and Momentum 0.8 to 0.7, Apply, the Rebuild model dialog, Play. |
| 6. Fire counts beside Game of Life's lines while playing | Pass. Lines 2 and 3 counted 662 and 656 at tick 38, then 56 and 56 once settled, kept after Pause. |
| 7. A sweep over `tick_length`, a note edit resumes, a rate edit is refused, Results and Open in Editor | Pass. 8 runs. After the note edit `--resume` skipped all 8 and `--reps 3 --resume` ran 4 more. After the rate edit `--resume` was refused on "different model code". `series.csv` has its `time` column, `henad-app --open` showed 12 runs, and Open in Editor opened the sheet with the note. |
| 8. Save, a fresh app, Convert, the HighLife edit, Show changes, Compare runs, Open original | Pass with two substitutions. Save and Open use native file dialogs the egui server cannot drive on a locked screen, so the starter in a freshly launched app stood in for the saved file. Line 2's Is constant went from 3 to 6 in place of the HighLife nodes, which the full check's tests cover. Show changes listed "Changed: Is (n2001)" under line 2, and Compare runs read "Game of Life (expert) first differs from Game of Life at tick 1: Alive is 11,318 against 22,455.". |
| 9. A look-around covering the world names its line, and the device survives | Pass after `802a4b2`. Boids at 200 by 200 with 70,000 birds and Visual range 200 reads "Lines 1 and 2, Flockmates and Too close: looks at too many birds at 70,000 birds. Use a shorter distance or fewer birds." in the CLI and in the app, and the preview played afterwards. The on-device fault test `a_capped_fold_fault_names_the_author_line` passes in the full check. |
| 10. The web build, and a sheet in the browser | Pass. `./scripts/build_web.sh build` passes in the full check. Headless Chrome on WebGPU against `build_web.sh serve` ran New model, SIR epidemic, Create and Play to "Tick 182 (182 days)" at 30 TPS with the epidemic curve. The server was stopped. |

Two items failed first, and each failure became a fix commit.

- **`b1a3106` (item 3).** The W10 fix put each state's Plot group inside `horizontal_wrapped`, and egui wraps no nested group. In the narrow sheet pane "Plot Recovered" sat at x 1155, past the card pane's edge at 1057, so a click missed and SIR's Recovered could not be plotted. Each state now takes one row. Test `every_plot_box_of_three_states_stays_in_the_sheet_pane_and_takes_a_click`, and in the app the boxes end by x 912.
- **`802a4b2` (item 9).** `SheetFile::shortfalls`, the cost mapping of S17, had no caller outside tests, so a sheet past the work budget named the lowered pass `boids_tick_bird_tick`. henad-cli's `run_single` now takes the `ModelFile` and uses the sheet's shortfalls, with no new crate edge. The app routes every shortfall through `AppState::entry_shortfalls`, which uses the Editor's sheet when its code hash equals the entry's, and the Parameters notice no longer doubles a reason's full stop. Tests `a_sheet_past_the_work_budget_is_refused_at_its_lines` (henad-cli) and `work_past_the_budget_on_a_sheet_names_its_lines_and_no_pass` (henad-app).

The app ran under the lock of plan section 2.4, with `app.ron` moving the window to G15F1 (1600 by 960 at -1800, 60).
The session's screen was locked, so every egui screenshot timed out, and the native items were driven and checked through the accessibility tree alone.
Afterwards the app was quit, `app.ron` restored and its backup deleted, the lock removed and port 5719 left free.
Item 10's screenshots and the run notes sit in the audit folder outside the repository.

### Docs and topic files

- `CHANGELOG.md`: the 0.4 lines of the redesign shortened (F7), the migration steps for the output writers and `ManifestModel` (deps-2, deps-3), and lines for each user-visible fix, including the sheet shortfalls line written with this record.
- `docs/reference/model-files.md`: the order inside a Repeat, the first Move to, Keep between, the ids that enter the code, a press's draws and small rates.
- `docs/guide/editor.md`, `docs/guide/sweeps.md`, `docs/guide/app.md` and `docs/developing/agent-record/agent-record.md`, as above.
- Record #92 item 7, corrected (F2).
- 17 of the 27 topic files under `agents/`, for the plan ids (F2), and the topic files of each area a fix touched.

### Edited tree

```
AGENTS.md                                      henad-graph's line names sheets
CHANGELOG.md                                   0.4 lines shortened, migration steps, audit fixes
agents/                                        plan and decision ids removed from 17 files, plus:
├── dependencies.md                            henad-models' dev edge to henad-graph
├── facade-and-tutorial.md                     henad::graph::normal
├── gpu-traps.md                               barriers of probed shaders
├── henad-app-editor.md                        Compare runs of an inexact model, entry_shortfalls
├── henad-app-sheet.md                         one Plot row per state
├── henad-app.md, henad-cli.md                 sheet shortfalls
├── henad-graph-sheet-lower.md                 Repeat order, first Move to, Keep between, press draws, small rates, start keys
├── henad-graph-wgsl.md                        barrier test
├── models.md                                  SIRD and presses in the grid lockstep
└── sim-runner.md                              GPU TPS cap, run_to_samples
crates/
├── henad/                                     normal re-exported, README and feature text name sheets
├── henad-app/src/
│   ├── lib.rs                                 run_to_samples into the chart history
│   ├── sim_runner.rs, state/build.rs          TPS commands after every GPU build
│   ├── state.rs, state/install.rs             entry_shortfalls
│   ├── tests/                                 expert, sheet_agents, sheet_pane tests, renames
│   └── ui/
│       ├── charts.rs                          legend in insertion order
│       ├── pacing.rs                          TPS controls for GPU models
│       ├── params.rs                          sheet shortfalls, one full stop
│       ├── playback.rs                        accessible names
│       └── editor/
│           ├── compare.rs                     CompareOutcome::Inexact
│           ├── mod.rs, preview.rs             PREVIEW_WAITS, no text positions
│           └── sheet/                         reserved names, thing words, help, Plot rows, refusal_text
├── henad-cli/
│   ├── src/lib.rs                             run_single uses the sheet's shortfalls
│   └── tests/model_file.rs                    sheet past the work budget
├── henad-compute/src/
│   ├── cpu/sim_thread.rs                      Run to tick samples
│   ├── gpu/sim_thread.rs                      TPS cap, Run to tick samples
│   ├── runner/mod.rs                          samples carried over a missed publish
│   └── snapshot.rs                            run_to_samples, MAX_RUN_TO_SAMPLES
├── henad-explore/src/handle.rs                SweepRun doc without a native-only link
├── henad-graph/
│   ├── Cargo.toml, README.md, src/lib.rs      sheets named
│   └── src/
│       ├── sheet/
│       │   ├── check/                         reserved names, empty_field, start on painted cells
│       │   ├── document.rs                    StartsAs::single_cells
│       │   ├── edit/                          is_reserved_name, starts_draws, old_state and new_state
│       │   ├── lower/
│       │   │   ├── colors.rs                  renamed from colours.rs
│       │   │   ├── ground.rs                  first Move to decides
│       │   │   ├── motion.rs                  Steers in line id order, Keep between doc
│       │   │   ├── rates.rs                   exact_chance_of, SMALL_HAZARD, 32-bit threshold
│       │   │   └── ...                        comments, names, limit sentences
│       │   ├── relations.rs                   inside_repeat, compare_test
│       │   └── sentence.rs                    new sentences, is_forbidden_word, thing words
│       └── tests/
│           ├── sheets/move_to_first.toml      new
│           └── ...                            identity, Repeat order, keys, rates, words tests
├── henad-models/
│   ├── Cargo.toml                             dev edge comment
│   └── src/tests/
│       ├── graph_probes.rs                    probed barriers in uniform control flow
│       ├── sheet_grid.rs                      SIRD and New outbreak in the lockstep
│       └── sheet_rates.rs                     a rate below a float draw's resolution
docs/
├── developing/agent-record/
│   ├── 20261107-92-ground-picks-and-fused-folds.md   item 7 corrected
│   ├── 20261108-105-redesign-audit.md         this record
│   └── agent-record.md                        redesign ids explained
├── guide/{app,editor,sweeps}.md               TPS cap, Compare runs, one tick length section
└── reference/model-files.md                   order rules, ids, press draws, small rates
zensical.toml                                  nav entry for this record
```

## Deviations

- **No screenshots of the native app.** The acceptance asks for the end-to-end list with screenshots. The screen was locked while the maintainer was away, egui screenshots time out on a locked screen, and unlocking needs the maintainer's password. The native items rest on the accessibility tree, and three visual checks are left (below).
- **Item 8's two substitutions**, named in the table above.
- **Fifteen fix commits.** Plan S32 names `fix: redesign audit [draft]`, one or more. Each of the fifteen adds its area to that summary.

## State after

- `37-node-editor` holds the fifteen fix commits and this record after `81278fc`, fast-forwarded from `redesign/s32-audit`. `37-node-editor` had not moved since the branch started, so nothing was rebased.
- The full check with `HENAD_REQUIRE_GPU=1` at `802a4b2` matches the baseline. Every step exits 0 but two, and both fail as the baseline does. `cargo deny` reports RUSTSEC-2026-0370, 0371 and 0372 and no other. The workspace tests pass 2,199 and fail one, `every_agent_corpus_file_at_its_largest_population_fits_the_work_budgets`, with no new failing name and no aborted test binary.
- clippy, the doc tests, the README program, `cargo doc`, the scripts' tests, the wasm32 checks and the web build pass.
- `python3 scripts/docs_rs.py --host` and `uv run zensical build --clean` pass.
- This record commit changes only the record, its nav entry and one CHANGELOG line, so the full check was not run again.

## Issues found & future directions

Confirmed findings left open, in full or in part:

- **The order of changes inside a Repeat** (F2). The lowering keeps the any-time rule, in which the higher line wins whichever time round each held. Round 1's "the first time it applies" would need a per-line minimum accumulator. It needs a decision row.
- **No "otherwise" for a lower Move to** (F5). The fall-through is fixed, but `change_key` covers Become and Set alone, and the app's otherwise tooltip and notice name a property, which a Move to lacks.
- **Chances at 24 bits** (G1). Chance lines, rates per element and competing groups still resolve a chance to 2^-24, and a rate per element still cancels to 0 below about 3e-8 per tick, as `model-files.md` says. The card shows no note for a chance below about 2^-20, which would need new wording-table text.
- **No agent lockstep** (D4). `vec.from_angle` and contracted products round differently on host and device, so an exact 20-tick lockstep of agent sheets would fail for reasons that are not bugs. A banded comparison could hold the agent half.
- **A press's draws follow its button's place** (D1). Keying the phase on the action id would change the action draws of every graph model.
- **W3.** After a sheet is applied, Apply of a new model still reads "Model applied. Press Build to run it.", and Play runs the old model until Build, as records #55 and #65 decided.
- **W2 in the browser.** A GPU Run to tick samples nothing on the way there, since the stats readback cannot block. The guide and `agents/sim-runner.md` say so.
- **Older long comment lines and British names** (F4, F12). Long lines from before the redesign stay. The pass names `colours` and `colours_ground` stay, since renaming them changes every sheet's code hash, and so does `SheetSection::Colours`, which mirrors the Colours heading.
- **UI-7's Blank description** stays as the gate words it.

Seen during the end-to-end run, not fixed:

- In a narrow sheet pane the Parameters rows run past the pane's edge, so clicks on their fields miss. A value typed through the accessibility focus works.
- New model replaces an unsaved sheet without asking.
- A sweep's plan in henad-explore (`probe.rs`) still words a budget refusal in the entry's terms, since it holds no `SheetFile`.
- `docs/assets/app/pacing-gpu.png` lacks the TPS controls W1 added.
- Not checked by eye: the legend order, the Charts after Run to tick, and the Measures rows. The maintainer can check all three in one run: New model, SIR epidemic, Create, Run to tick 200, then the Charts tab.

Wording to add to gate section 10 at the next gate: `runs_do_not_replay`, `PREVIEW_WAITS`, `empty_field`, `name_reserved`, the three size-limit sentences, the work-budget imperatives, "Model unchanged." and "Start new model".

## Decisions adopted for review

The audit followed these gate rows.

| Id | Topic | Sources | Decision | Milestones |
| --- | --- | --- | --- | --- |
| D60 | The words rule's reach | gate section 10 | Applied to the UI and walk findings. W6 is rejected under it, and the Pacing tab's TPS controls keep their own words | S32 |
| D74 | Hand-set cells | `CHANGES-A.md` F1 | Followed. The blinker and glider checks stay headless, and item 2 uses a random start | S32 |
| D86 | Run controls | `CHANGES-A.md` F21, `CHANGES-B.md` B4 | Its premise now holds for sheets: W1 makes Pacing's Unlimited TPS work on GPU models, and W2 gives Run to tick a whole run on the Charts tab | S32 |
| D94 | Findings judged simulation artifacts | `CHANGES-C.md` C6 | Its premise, "the default speed shows the SIR wave", held for no sheet until W1. With a TPS cap set, Play now shows the wave | S32 |

The audit also chose these in the maintainer's absence. None has a gate row yet.

| Topic | Finding | Choice |
| --- | --- | --- |
| Order of changes inside a Repeat | F2 | The any-time rule stays and is documented. Round 1's first-time rule waits for the maintainer |
| A result worked out inside a Repeat | F1 | It is no single value per element, so changes that test it may both apply, and display order decides |
| Two Move to lines | F5 | The first whose condition holds decides, as round 1 section 6 says, and a blocked place keeps the agent still |
| Keep between and Set | F4 | The docs follow the lowering: a Keep between holds the value after every Set of its property |
| Order of Steers | L2-F1 | Steers between two Keep betweens add up in line id order, so their display order stays outside the code |
| Small rates | G1 | A rate the same for every element draws through a 32-bit threshold below a hazard of 2^-10. Chance lines keep the engine's 24 bits |
| A start on several painted cells | D3 | It draws, and takes a key |
| Compare runs | D2 | A model that does not replay exactly names no first difference |
| GPU pacing | W1 | The GPU runner starts uncapped and keeps to the target TPS once the app sends `SetUncapped(false)` |
| Run to tick | W2 | Up to 500 samples per run, a blocking readback per sample on native GPU, none in the browser |
| A press's draws | D1 | Documented, keyed by the button's place as before |

## Benchmark commands left for the maintainer

No benchmark was run.
Run each set interleaved, round by round, on a cooled machine, as `agents/performance.md` asks.
Every input below exists at `802a4b2`.
Records #71, #73, #75 to #80, #82, #84 to #86, #88 to #90, #93 and #96 to #103 left none.
G1 changes the SIR starter's code, so #91's and #94's SIR runs measure the threshold form.

1. **#72 (workgroup counters).** Build a `f4088d7` worktree and the current henad-cli. Over 5 rounds run `--steps 2000 --reps 5 --json` on `gpu_boids`, `crates/henad-models/tests/fixtures/graphs/gpu_boids.toml` and `crates/henad-graph/starters/boids.toml`, then on `gpu_ants`, `gpu_ants.toml` and `starters/ants.toml` with `--set num_agents=1000000 --set world_width=1000 --set world_height=1000`, old binary against new, and compute the median ratio over the stock port with record #72's script. It needs about 12 GB free.
2. **#74 (fire-count channel).** Build record #74's standalone `fire-count-bench` crate, which compiles the Game of Life graph starter plain and probed, and run `cargo run --release -- 4096` for 5 rounds at batch 1 and batch 64.
3. **#81 and #83 (Game of Life sheet against its graph starter).**
   ```bash
   cargo run --release -p henad-cli -- --model-file crates/henad-graph/sheets/game_of_life.toml --set grid_width=4096 --set grid_height=4096 --steps 200 --reps 3
   cargo run --release -p henad-cli -- --model-file crates/henad-graph/starters/game_of_life.toml --set grid_width=4096 --set grid_height=4096 --steps 200 --reps 3
   ```
4. **#87 (boids sheet against its graph starter).**
   ```bash
   for i in 1 2 3; do
     cargo run --release -p henad-cli -- --model-file crates/henad-graph/sheets/boids.toml --set num_agents=200000 --steps 500 --reps 3
     cargo run --release -p henad-cli -- --model-file crates/henad-graph/starters/boids.toml --set num_agents=200000 --steps 500 --reps 3
   done
   ```
5. **#91 (rates, chance SIR and SIRD).**
   ```bash
   for rep in 1 2 3; do
     cargo run --release -p henad-cli -- --model-file crates/henad-graph/sheets/sir.toml --set grid_width=4096 --set grid_height=4096 --steps 200 --reps 1 --json
     cargo run --release -p henad-cli -- --model-file crates/henad-models/tests/fixtures/sheets/sir_chance.toml --set grid_width=4096 --set grid_height=4096 --steps 200 --reps 1 --json
     cargo run --release -p henad-cli -- --model-file crates/henad-models/tests/fixtures/sheets/sird.toml --set grid_width=4096 --set grid_height=4096 --steps 200 --reps 1 --json
   done
   ```
6. **#92 (ants sheet against `gpu_ants` and the ants graph starter).**
   ```bash
   for i in 1 2 3; do
     cargo run --release -p henad-cli -- --model-file crates/henad-graph/sheets/ants.toml --set num_agents=1000000 --steps 500 --reps 3
     cargo run --release -p henad-cli -- gpu_ants --set num_agents=1000000 --steps 500 --reps 3
     cargo run --release -p henad-cli -- --model-file crates/henad-graph/starters/ants.toml --set num_agents=1000000 --steps 500 --reps 3
   done
   ```
7. **#94 (optional, live parameters).**
   ```bash
   for rep in 1 2 3; do
     cargo run --release -p henad-cli -- --model-file crates/henad-graph/sheets/sir.toml --set grid_width=4096 --set grid_height=4096 --steps 200 --reps 1 --json
   done
   ```
8. **#95 (lane packing).** Write `/tmp/ants_energy.toml` with record #95's Python snippet, which adds an Energy property and line l17, then:
   ```bash
   for rep in 1 2 3; do
     cargo run --release -p henad-cli -- --model-file crates/henad-graph/sheets/ants.toml --set num_agents=1000000 --steps 500 --reps 1 --json
     cargo run --release -p henad-cli -- --model-file /tmp/ants_energy.toml --set num_agents=1000000 --steps 500 --reps 1 --json
   done
   ```
9. **#104 (counting cost).** Build `/tmp/henad-cli-counting`, then `/tmp/henad-cli-silent` with `sheet_probes` returning empty probes through record #104's `sed` and `printf` on `crates/henad-explore/src/model_file.rs`, restored from `.bak` afterwards. Over 5 rounds run each build on `--model-file crates/henad-graph/sheets/game_of_life.toml --set grid_width=4096 --set grid_height=4096 --steps 2048 --reps 3`. #74's program covers the app's one tick per snapshot.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
