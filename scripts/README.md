# Henad `scripts/`

Analysis and comparison scripts.

- `bench_matrix.py`: sweeps every registered model across the configuration matrix into a CSV
- `plot_bench_history.py`: figures from that CSV
- `compare_sir.py`: cross-engine SIR comparison, Henad against a reference engine

```bash
uv run --project scripts scripts/compare_sir.py --netlogo path/to/netlogo_csvs --generate 50
```
