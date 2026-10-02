# Recording the 0.2.0 sweep folder

Produces the sweep folder beside this file: `manifest.json`, `runs.csv`, `series.csv` and `summary.csv`.
`a_0_2_0_manifest_still_resumes` (`crates/henad-explore/src/tests/provenance.rs`) copies the folder, resumes it twice with more replicates, and checks the warning each resume gives, the builds the folder records, and that the resumed runs equal a fresh sweep's.

## Why the reference is Henad itself

Every result folder Henad 0.2.0 wrote has to keep resuming, merging and replaying.
The reference is therefore what 0.2.0 wrote, and the rule against fixtures made by Henad does not apply, since that rule covers correctness oracles.
The test reads the timings, the adapter, the host, the command line and the clock only to compare builds.

## Procedure

Record the folder from the `v0.2.0` tag, never from a changed tree.
A tree that has already changed the manifest or a hash would record its own format, and the test would pass against it.

From the repository root:

```bash
git worktree add --detach ../henad-0.2.0 v0.2.0
cd ../henad-0.2.0
git log --oneline -1          # 773a7a5 release: bump version to 0.2.0
cargo build --release --locked -p henad-cli
rm -rf ../henad/crates/henad-explore/tests/fixtures/manifest-0.2.0/*.json \
    ../henad/crates/henad-explore/tests/fixtures/manifest-0.2.0/*.csv
target/release/henad-cli sir --set grid_width=16 --set grid_height=16 --vary infection_rate=0.2,0.4 \
    --act seed_outbreak@5 --reps 2 --steps 20 --stats-every 5 --seed 7 \
    --out ../henad/crates/henad-explore/tests/fixtures/manifest-0.2.0
target/release/henad-cli --info
cd - && git worktree remove ../henad-0.2.0
```

The `--out` path assumes the checkout sits in a directory named `henad`. Adjust it to the checkout's path.
The sweep runs 2 configs of SIR with 2 replicates each, 4 runs, on the CPU, and fires an outbreak at tick 5.
`--info` names the machine and the adapter.

## Recorded

Recorded on 2026-10-02 on an Apple M4 Pro (macOS, aarch64, 14 logical CPUs) with its Metal adapter, from a release build at 773a7a5 on rustc 1.97.1.
The manifest's engine block reads version 0.2.0 and commit `773a7a5b`, and its one session reads commit `773a7a5b`.
