# Recording the 0.2.0 schema hashes

Produces the constants `SCHEMA_HASHES_0_2_0` in `crates/henad-explore/src/schema.rs`, which `schema_hashes_are_unchanged_since_0_2_0` compares every example model's current `schema_hash` against.

## Why the reference is Henad itself

A sweep folder records its model's `schema_hash`, and a resume or a merge refuses a folder whose hash differs from the running model's.
The hash covers the model's id and its parameter, stat and action descriptors (`henad_core::explore::fingerprint::schema_hash`).
The test pins backward compatibility: a folder Henad 0.2.0 wrote has to keep resuming.
The reference is therefore what 0.2.0 wrote, and the rule against fixtures made by Henad does not apply, since that rule covers correctness oracles.

## Procedure

Record the hashes from the `v0.2.0` tag, never from a changed tree.
A tree that has already changed a descriptor would record its own hash, and the test would pass against it.

From the repository root:

```bash
git worktree add --detach ../henad-0.2.0 v0.2.0
cd ../henad-0.2.0
git log --oneline -1          # 773a7a5 release: bump version to 0.2.0
cargo build --release --locked -p henad-cli
for id in $(target/release/henad-cli --list | awk 'NR > 1 { print $1 }'); do
    target/release/henad-cli "$id" --steps 1 --out "/tmp/schema-0.2.0/$id"
    python3 -c "import json, sys; m = json.load(open(sys.argv[1])); print(sys.argv[2], m['model']['schema_hash'])" \
        "/tmp/schema-0.2.0/$id/manifest.json" "$id"
done
target/release/henad-cli --info
cd - && git worktree remove ../henad-0.2.0
```

Each run is a one-run sweep, and `model.schema_hash` in its `manifest.json` is the hash 0.2.0 records.
`--list` names a GPU model only where a compute adapter exists, so record on a machine with one.
`--info` names the machine and the adapter.

## Recorded

Recorded on 2026-10-01 on an Apple M4 Pro (macOS, aarch64, 14 logical CPUs) with its Metal adapter, from a release build at 773a7a5 on rustc 1.97.1.
Each manifest's engine block read version 0.2.0 and commit `773a7a5b`.

| Model | `schema_hash` |
|---|---|
| `sir` | `6ff1dc3971fd0a96` |
| `boids` | `ae99f3e0d37c8d3d` |
| `game_of_life` | `aba7303a467b06bf` |
| `ants` | `f6eb31e76efdf4cf` |
| `virus_network` | `77993c7b4047b8bd` |
| `team_assembly` | `d1724ce65dada5d6` |
| `gpu_game_of_life` | `461ad8e0d063a398` |
| `gpu_sir` | `99f14d30367e3743` |
| `gpu_boids` | `f7328729019009c0` |
| `gpu_ants` | `b58e8a5a5b6a829e` |
