---
date: 2026-08-15
title: "Naming and American-English spelling rules in AGENTS.md, applied to code identifiers"
model: grok-4.5 (timi-cc-grok/grok-4.5), via opencode
issue: none — user request to record two style rules and sweep the codebase
status: complete — rules written, identifier/string/WGSL violations fixed, comments left British
baseline_commit: 82775b8
delta_state: uncommitted working tree on `master`
---

# Naming and American-English spelling

> Two style rules were added under **Writing style** in `AGENTS.md`:
> American English for code identifiers and string literals (comments may stay British),
> and Rust API Guidelines naming.
>
> The codebase was then checked against both.
> Only code identifiers, string literals, and WGSL sources were changed.
> British spellings in comments were left alone on purpose.

---

## State before

At `82775b8` on `master` (comment-style cleanup already landed).
`AGENTS.md` had no spelling rule and no pointer to the Rust API naming page.
Code mixed American and British forms in identifiers (`color` fields next to `colour` locals, `quantize` nowhere but `quantise` as a trait method).

## What was done

### `AGENTS.md`

Under **Writing style**, before **Comments and doc comments**:

- **Spelling** — code uses American English (`color`, `center`, `optimize`, `quantize`); comments may stay British.
- **Naming** — link to https://rust-lang.github.io/api-guidelines/naming.html (C-CASE, C-CONV, C-GETTER, etc.).

### Code fixes (identifiers, strings, WGSL only)

| change | where |
| --- | --- |
| `StatsHistory::get_tick` → `tick` (C-GETTER) | `henad-core/src/view.rs` |
| local `colour` → `color` | `henad-app/src/ui/params.rs` |
| `ScalarFieldSpec::quantise` → `quantize` | `henad-compute/src/cpu/field/scalar.rs`, `henad-models/src/ants/field.rs` |
| local `centre` → `center`, assert message | `henad-models/src/boids/step.rs` |
| assert message `colour` → `color` | `henad-compute/src/cpu/sim_thread.rs` |
| assert message `quantised` → `quantized` | `henad-models/src/ants/mod.rs` |
| WGSL `centre` / `colour` / `*_COLOUR` → American | `agents.wgsl`, `gpu_sir/display.wgsl`, `gpu_game_of_life/display.wgsl`, `gpu_ants/display.wgsl` |

Comment-only British spellings (`colours`, `centres`, `optimisation`, `quantised` in docs, etc.) were **not** rewritten.

Edited tree:

```
AGENTS.md
crates/
  henad-app/src/ui/{agents.wgsl,params.rs}
  henad-compute/src/cpu/{field/scalar.rs,sim_thread.rs}
  henad-core/src/view.rs
  henad-models/src/
    ants/{field.rs,mod.rs}
    boids/step.rs
    gpu_ants/display.wgsl
    gpu_game_of_life/display.wgsl
    gpu_sir/display.wgsl
docs/agent-record/20260815-07-naming-and-spelling.md
```

## State after

- `cargo check --workspace --all-targets` clean.
- `cargo test -p henad-core --lib` and `cargo test -p henad-models --lib` (62 tests) pass.
- No remaining British forms in identifiers or string literals under `*.rs` / `*.wgsl`.
- Comments still freely use British English.

## Issues found & future directions

**1. `get` on `StatsHistory` stays.**
`get(col, j)` is the bounds-checked indexed form, matching the C-GETTER exception for `get`/`get_unchecked`-style APIs.
Only the redundant `get_tick` prefix was wrong.

**2. Conversion prefixes already look fine.**
`as_gpu_mut` is the only project `as_`/`to_`/`into_` method found; it matches C-CONV.

**3. Icon constant names (`MS_*`, `MDI_*`) are external glyphs, not ours to rename.**

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     If you update this document, stop at the line above.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
