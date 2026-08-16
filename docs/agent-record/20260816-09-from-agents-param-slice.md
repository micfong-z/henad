---
date: 2026-08-16
title: "Pass model parameters through from_agents"
model: gpt-5.6-terra (OpenCode)
issue: "PR #35 review follow-up"
status: complete
baseline_commit: 1f21c9e
delta_state: uncommitted working tree on `33-wgsl-bindgen`
---

# `from_agents` parameter slice

> `AgentModelState::from_agents` now gives `AgentModel::init` the model-owned parameter slice.
> A boids regression test protects custom-lane construction from using the composed engine parameter list.

## State before

The parameter-index change on PR #35 made model parameter constants local to each model-owned descriptor list.
`AgentModelState::from_params_seeded` already split the composed list before calling `AgentModel::init`.
`AgentModelState::from_agents` still passed the full composed list to `init`.
For boids, that made `MAX_SPEED` and `MIN_SPEED` resolve to separation and alignment when the callback only replaced positions.

## What was done

- Split the composed list before `from_agents` calls `AgentModel::init`.
- Passed `own` to `init` and retained the field slice for field allocation.
- Added a boids test that overrides only the initial position and asserts the initialized velocity uses the configured speed band.
- Ran both affected library test suites and formatting checks.

### Edited codebase structure

```
crates/
├── henad-compute/
│   └── src/
│       └── cpu/
│           └── agent_engine.rs           ~ split composed params before `from_agents` initialization
└── henad-models/
    └── src/
        └── boids/
            └── mod.rs                    ~ regression coverage for seeded velocities
docs/
└── agent-record/
    └── 20260816-09-from-agents-param-slice.md  + session hand-off
```

## State after

Both agent-state constructors pass model-local parameters to `AgentModel::init`.
`from_agents` callbacks can override a subset of initialized lanes without inheriting initialization values from the wrong composed-list offsets.

Verification completed with `cargo test -p henad-models --lib`, `cargo test -p henad-compute --lib`, `cargo fmt --all -- --check`, and `git diff --check`.

## Issues found & future directions

The defect was found by the PR #35 review after the parameter-index migration.
No further mismatched `AgentModel::init` call sites were found.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

Can confirm that this is a valid issue.
Manually added a epsilon for float comparison.
