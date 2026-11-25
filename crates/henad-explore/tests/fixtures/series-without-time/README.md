# Recording the sweep folder without a `time` column

Produces the sweep folder beside this file: `manifest.json`, `runs.csv`, `series.csv` and `summary.csv`.
`a_folder_written_before_the_time_column_still_reads` (`crates/henad-explore/src/tests/result_set.rs`) opens it and reads its series.
`a_series_with_a_time_column_reads_by_its_header` adds a `time` column to its `series.csv` and checks that the series read the same.
Both fail when the folder is missing.

## Why the reference is Henad itself

`series.csv` gained a `time` column after `tick` for a model with a time scale, and the reader now takes its id columns from the header.
Every folder written before that change has to keep reading.
The reference is therefore what the build before the change wrote, and the rule against fixtures made by Henad does not apply, since that rule covers correctness oracles.

## Procedure

Record the folder from commit `79e6d1c` of the `37-node-editor` branch, never from a changed tree.
A tree that already writes the new header would record its own format, and the test would pass against it.

From the repository root:

```bash
mkdir -p /tmp/henad-s2-79e6d1c
git archive 79e6d1c | tar -x -C /tmp/henad-s2-79e6d1c
(cd /tmp/henad-s2-79e6d1c && CARGO_TARGET_DIR=/tmp/henad-s2-79e6d1c-target cargo build --release -p henad-cli)
rm -f crates/henad-explore/tests/fixtures/series-without-time/*.json \
    crates/henad-explore/tests/fixtures/series-without-time/*.csv
/tmp/henad-s2-79e6d1c-target/release/henad-cli sir --set grid_width=16 --set grid_height=16 \
    --vary infection_rate=0.2,0.4 --reps 1 --steps 10 --stats-every 5 --seed 7 \
    --out crates/henad-explore/tests/fixtures/series-without-time
rm crates/henad-explore/tests/fixtures/series-without-time/.lock
rm -rf /tmp/henad-s2-79e6d1c /tmp/henad-s2-79e6d1c-target
```

The sweep runs 2 configs of SIR with 1 replicate each, 2 runs of 10 steps on the CPU, sampled every 5 ticks.
The `.lock` file the writer leaves is removed, since a reader never takes the lock.
A tree extracted with `git archive` holds no `.git` directory, so the build stamps no commit and the manifest's `commit` fields are empty.
The manifest's `argv` names the binary and this folder as they were when it was recorded, under the hash the commit had then (`/tmp/henad-s2-be338ec-target/...` and `series-be338ec`), and no test reads it.

## Recorded

Recorded on 2026-11-07 on an Apple M4 Pro (macOS, aarch64, 14 logical CPUs), from a release build of `79e6d1c` (version 0.3.0) on the workspace's pinned toolchain.
