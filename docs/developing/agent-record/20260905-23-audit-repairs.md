---
date: 2026-09-05
title: "Repairing the cross-engine benchmarks after a fourteen-auditor review"
description: An adversarial audit of the issue 25 branch confirmed forty-four findings, and this session fixes them: the gate now gates, GPU reps stop being process-cold, the lines-of-code table stops counting Henad's own tests, and the published prose stops contradicting the code.
icon: material/note-text-outline
status: ai-generated
model: claude-opus-5 (Claude Code)
issue: 25 — Benchmarks against other ABMs
state: every confirmed audit finding fixed, no sweep re-run, published numbers marked as needing regeneration
baseline_commit: 33dce62
delta_state: uncommitted on `25-cross-abm-comparison`
---

# Repairing the cross-engine benchmarks

> Fourteen auditors partitioned the branch that closes issue 25, and every finding they raised at critical, high or medium was put through independent refuters.
> Forty-four survived, of which one fell to a refuter and sixty-four more were logged as minor.
> The ports themselves came out clean: all twenty-five committed fixtures regenerate byte for byte.
> What was wrong was the apparatus around them, and this session fixes it.

## State before

The branch had all five reference engines ported and all twenty gates passing, verified by hand.
Nothing was wrong with the simulations.

Three things were wrong with everything else.
`compare_bench.py` never read `validated.json`, so six documents describing a refusal no code performed.
`henad-cli` seeded every rep identically, which forced the driver to run one process per rep, which put GPU ramp-up inside every timed window.
And the published prose had drifted, most visibly an admonition saying four ports were not written sitting three paragraphs above their gate verdicts.

## What was done

### The gate gates

`validate_ports.py` used to hand the tracked fixture path straight to a harness as `--out` and unlink on failure.
Since the comparison is directory-wide, one disagreeing engine took the correct engines' fixtures down with it, and `cargo test` stayed green afterwards while checking fewer engines.
Candidates now go to a temporary tree, `HENAD_FIXTURE_DIR` points the consistency tests at it, and a fixture reaches the repository only after it passes.
That fixes the attribution too, since only one engine's candidates are present when a comparison fails.

`compare_bench.py` reads the verdicts and skips anything that is not `yes`, unless `--allow-unvalidated` says otherwise, and the verdict lands in the CSV column that used to be empty on every row.
Proved by breaking Mesa's separation sign: the gate reported the exact disagreeing agent, every committed fixture stayed in place, and the sweep then declined to time Mesa.

Every variant is gated rather than only the first, so krABMaga's `parallel` build is no longer timed on the default build's verdict.
The SIR half of that needed its replicate directory keyed on the variant too, since otherwise the second variant found the first's fifty files already written and was recorded as passing a gate its harness never ran.
The SIR replicate cache is keyed on a hash of the harness, since an edited port used to keep its verdict forever.
And each gate now asserts the fixture's declared size and step count against the scenario, rather than re-running Henad at whatever the candidate's own header said. Boids checks its world only when the header carries one, the two original NetLogo fixtures predating that field.

### One seed per rep, and the GPU stops being cold

`bench_cpu` and `bench_gpu` passed `args.seed` unchanged inside the rep loop, against a protocol that fixes rep `i` at `base + i`.
The driver worked around it by running a process per rep, and that workaround is what charged every GPU rep for shader compilation and a cold clock.

Seeding properly removes the workaround, and the driver then passes `--global-warmup` on the GPU variant to ramp the device before rep 0.
Both halves are needed: seeding alone still leaves the first rep cold.
Measured at 1024 squared with the ramp, reps read 2.56, 2.43, 2.68, 2.39, 2.35 ms where the first used to be 6.22.

The curves also moved onto the median, which is what the tables beside them already used.
A dirty tree now marks the commit stamp, since the published sweep names a commit whose binary accepts neither flag the driver passes.

### The lines-of-code table, in both directions

The counter applied one comment rule to six languages, so Python and Julia paid for their docstrings while every other language's comments came out free, and Rust attributes counted as comments.
It also counted the CDATA wrapper in every NetLogo model, which is why the page's prose said 52 beside a table saying 54.
And Henad's rows counted its own inline `#[cfg(test)]` blocks, which no port file has.

Counting by the language, Henad's Game of Life falls from 121 to 57 and NetLogo's lands on the 52 the prose always claimed.

### What the ant rungs actually measure

Probed on this branch: at 2000 agents the first delivery lands at tick 900, and at 20000 the first ant picks food up at tick 4250 with none delivered by tick 12000.
The ladder times 200 ticks.
So no published ant rung reaches the food, `to_food` stays zero, and what is timed is a random walk over a flat field plus a full-grid decay pass.

The nest-to-food gap grows with the world and the world grows with the population, so reaching the trail-following regime is unaffordable at the larger rungs in any step count an engine here could run.
The ladder is left alone and the page says what the rung measures.

### Henad's obstacle map moves to the declaration

Henad built the ants site map in `f32` with the multiply distributed, where the declaration and four of five ports use `f64` and group the sums first.
The maps agree at 32 and at 200 and disagree at 231 widths between 801 and 3000, so the shipped ladder never saw it.

Henad now matches the declaration, and so does krABMaga, which had copied Henad's form.
The formula lives in four places that had to move together, the shipped model, the tutorial copy a parity test pins against it, the tutorial guide's literal code block, and the declaration.
Every fixture is unchanged, as the agreement at 32 predicted.

### Prose that now matches the code

The work-in-progress admonition said four ports were not written; it now says all five are written and gated and none is timed, and carries the three reasons the existing numbers need regenerating.
The gate table said five ticks where the code uses four.
Three documents credited NetLogo's timed SIR with a repaint its `go` does not contain, and called both NetLogo models byte copies when each file's own header lists what changed.
The record for session 22 is corrected in place, its human notes untouched.

`sir_fixture.md` had a Henad column that no longer reproduced from the command the same document gives.
Re-run, it is 0.02990, 11.04 and 0.32560, which is what the branch's own later table already said.
The spread and headroom tables were re-measured with it.

New prose was hard-wrapped against the semantic-line-break rule, mixed with sections that were not.
Reflowed to one sentence per line across five files, verified word-for-word identical.

### CI builds the ports

Nothing in CI compiled a port, so inverting a sign in MASON left it green.
A `ports` job now checks krABMaga under both features, imports every Mesa model, instantiates Agents.jl, and smoke-runs a tiny benchmark on each.
MASON and NetLogo need a jar CI does not have, and `benchmarks/README.md` says so rather than implying coverage.

```text
.github/workflows/ci.yml                          + the ports job
AGENTS.md                                         gate claim, and the fixture rule's engine list
benchmarks/
    README.md                                     NetLogo notes, CI coverage, disclosed asymmetries
    protocol.md                                   Henad's exception stated, enforcement described
    agents_jl/src/boids.jl                        the spacing fallback fails loudly
    krabmaga/
        Cargo.toml                                cargo's own release defaults
        src/main.rs                               `sci` writes a bad number instead of panicking
        src/models/ants.rs                        obstacle map, and deliveries counted
    mesa/bench.py                                 rejects a parameter it does not have
crates/
    henad-cli/src/
        main.rs                                   per-rep seeds, probe drop, `--reps` floor
        json_report.rs                            median matches the driver's
    henad-models/
        src/ants/field.rs                         obstacle map in f64
        src/tests/tutorial/foraging/field.rs      the same, kept in parity
        tests/consistency_*.rs                    declared configuration asserted, fixture dir override
        tests/fixtures/docs/                      ants and sir declarations corrected
docs/
    benchmarks.md                                 seven corrections and a staleness notice
    guide/first-model/ants.md                     the tutorial's copy of the formula
    reference/cli.md                              the JSON example, and the threads example
    developing/agent-record/
        20260904-22-cross-engine-benchmarks.md    stale numbers corrected in place
        20260905-23-audit-repairs.md              this file
scripts/
    compare_bench.py                              gate enforcement, seeds, resume, timeouts
    validate_ports.py                             staged fixtures, variants, SIR cache, CLI
    compare_sir.py                                inconclusive is exit 2, and `--binary`
    plot_compare.py                               language-aware counting, cells, styles
    progress.py                                   plain mode keeps the median
zensical.toml                                     (table fragments renamed instead)
```

## State after

`./check.sh` is green and `HENAD_REQUIRE_GPU=1 cargo test --workspace --all-targets --all-features` passes.
Every committed fixture is byte-identical to what its port produces now, checked for Mesa and krABMaga directly and for all five through `cargo test`.

The gate is enforced, and breaking a port demonstrates it end to end.
The published tables are regenerated where generation was wrong, and marked as needing a fresh sweep where only a sweep can fix them.

The six generated table fragments are `.snippet` rather than `.md`, so they stop building as standalone search-indexed pages. `exclude_docs` is not honoured by Zensical, which is why the rename rather than a config key.

## Issues found & future directions

- **No reference engine has ever been timed.** The published CSV is 54 Henad rows. The whole timing half of issue 25 is unrun, and the issue should not close before it is.
- **The ladder is probably not survivable by the slower engines.** A probe put Mesa's boids at about five times the cost for four times the agents, so the 1000 second budget will stop it early. Sizing the ladder against the engines rather than against Henad is the first thing the next session should do.
- **Two cross-engine comparisons were never made.** Only ants was compared five ways side by side. Boids and the two grid models were each audited port by port against Henad, and the boids gate is one tick at parameters the ladder does not use, so a divergence on the timed path would not be caught.
- **The ants gate reaches less of the model than it looks.** Momentum, random action, the 1e-14 floor, `decode_step` and the nest-delivery arm are all unreachable in the gate scenario. A second deterministic scenario would cover them and is not written.
- **Mesa disables the cyclic collector inside its timed window** where MASON and NetLogo let theirs run, and MASON gets no JVM heap flag where NetLogo gets one. Both are disclosed rather than equalised, and either choice favours somebody.
- **`heap_bytes` means four different things** across the harnesses and is collected into the CSV anyway. It is never plotted, so nothing has gone wrong yet.
- **Sixty-four minor findings were logged and only some were acted on.** They were reported without verification, and the ones left are recorded in the audit rather than in the code.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

<!-- Seeded by the agent: what the human did this session, from the agent's point of view.
     Raw material to reframe, not notes. Delete this block once rewritten.

     - Asked for the audit to be run with adversary agents under ultracode, which is what produced
       the fourteen-auditor fan-out and the three-refuter verification behind every finding here.
     - Caught that resuming the audit workflow was re-running agents that had already succeeded and
       burning quota, and asked for it to be restructured so completed work was not repeated. That
       is why the second run reads its inputs from a file instead of re-deriving them.
     - Chose code and docs only, no sweep, with the published numbers marked stale rather than
       regenerated. That decision is what keeps this session bounded and pushes the real comparison
       to its own run.
     - Chose to raise the ants step count so the rung reaches the trail-following regime, then
       reversed to disclosing the regime once the probe showed the first delivery lands at tick 900
       at the smallest rung and past tick 12000 at the middle one. The measurement changed the
       decision, which is the reason it was worth taking before editing the ladder.
     - Chose Henad's obstacle map to move to the declaration rather than the declaration to Henad,
       which is what kept the four non-Rust ports untouched.
     - Chose to correct record 22 in place rather than leave it and write a new one.
-->
