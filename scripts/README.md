# Henad `scripts/`

Analysis and comparison scripts.

- `bench_matrix.py`: sweeps the registered models across the configuration matrix into a CSV, skipping `team_assembly` unless asked
- `plot_bench_history.py`: figures from that CSV
- `compare_sir.py`: cross-engine SIR comparison, Henad against a reference engine
- `compare_network.py`: the same comparison for Virus on a Network and Team Assembly, one statistics table per model
- `compare_bench.py`: sweeps every installed engine across the cross-engine ladder into a CSV
- `validate_ports.py`: checks each reference implementation against Henad before anything is timed
- `plot_compare.py`: figures and tables from that CSV, for the benchmarks page
- `changelog_section.py`: prints one version's `CHANGELOG.md` section, which the release workflow uses as the release body
- `docs_rs.py`: builds each published crate's documentation as docs.rs builds it, from the crate's `[package.metadata.docs.rs]` table, for the `docs` CI job
- `ci_tiers.py`: runs `cargo test` for CI's quick or full tier, prints the jobs a change needs, and checks that every line of `slow_tests.txt` matches exactly one test
- `slow_tests.txt`: the slow tests that CI's quick tier and `./check.sh --quick` leave out
- `check_packaging.sh`: checks the packaging rules that cargo never checks, such as workspace versions, licence copies and the template's requirements, for `./check.sh` and CI
- `build_web.sh`: builds or serves the web target on the pinned nightly, the only supported web build

`changelog_section.py`, `ci_tiers.py` and `docs_rs.py` need no dependencies, and CI runs them with a bare `python3`.
Their tests run in CI as `python3 -m unittest discover -s scripts -p 'test_*.py'`.

`progress.py` is a helper rather than a script: the live display the sweep draws while it runs, which falls back to one line per run whenever output is redirected.

```bash
uv run --project scripts scripts/compare_sir.py --reference path/to/reference_csvs --generate 50
uv run --project scripts scripts/compare_network.py virus_network --reference path/to/reference_csvs --generate 400
uv run --project scripts scripts/compare_bench.py --dry-run
```

See `benchmarks/README.md` for the cross-engine comparison as a whole.
