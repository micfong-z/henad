# Henad `scripts/`

Analysis and comparison scripts.

- `bench_matrix.py`: sweeps every registered model across the configuration matrix into a CSV
- `plot_bench_history.py`: figures from that CSV
- `compare_sir.py`: cross-engine SIR comparison, Henad against a reference engine
- `compare_bench.py`: sweeps every installed engine across the cross-engine ladder into a CSV
- `validate_ports.py`: checks each reference implementation against Henad before anything is timed
- `plot_compare.py`: figures and tables from that CSV, for the benchmarks page

`progress.py` is a helper rather than a script: the live display the sweep draws while it runs, which falls back to one line per run whenever output is redirected.

```bash
uv run --project scripts scripts/compare_sir.py --reference path/to/reference_csvs --generate 50
uv run --project scripts scripts/compare_bench.py --dry-run
```

See `benchmarks/README.md` for the cross-engine comparison as a whole.
