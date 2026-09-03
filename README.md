<img width="100%" alt="banner" src="https://github.com/user-attachments/assets/9d277d98-19ba-457d-b51c-8560a1251818" />

---

**Henad** is a very fast agent-based modelling engine that aims to be the most powerful and flexible ABM engine on personal computers.

Try Henad on https://henad.micfong.space

> [!warning]
> CPU models run noticably slower on WASM. GPU models appear to have similar performance compared to native builds.
>
> Consider running Henad natively for maximum performance.

## WIP Screen Recording (2026-08-01)

<img width="800" height="450" alt="GIF screen recording" src="https://github.com/user-attachments/assets/a64ffa6d-2d96-4fe7-9351-ee9e7810b751" />

https://github.com/user-attachments/assets/7ee3fadb-a8fa-4b79-84fa-7b4cd4099f23

## Benchmarks

Benchmarks were run on Ubuntu 20.04.6 LTS x86_64 with AMD Ryzen Threadripper 3960X and NVIDIA GeForce RTX 4090. Each entry was run 3 times and the mean was taken.

_Agent-Based Models_

| #   | Model                | Agents    | Total Steps | Time (s) | Steps / s | Agent Updates / s |
| --- | -------------------- | --------- | ----------- | -------- | --------- | ----------------- |
| 1   | [CPU] Ant Foraging   | 1,000     | 1,000       | 1.16     | 862.17    | 862,174.54        |
| 2   | [CPU] Ant Foraging   | 1,000,000 | 1,000       | 134.20   | 7.45      | 7,451,525.79      |
| 3   | [CPU] Boids Flocking | 1,000     | 1,000       | 6.99     | 143.15    | 143,147.12        |
| 4   | [CPU] Boids Flocking | 50,000    | 1,000       | 31.32    | 31.93     | 1,596,674.07      |

_Grid-Based Models_

| #   | Model              | Grid Size | Total Steps | Time (s) | Steps / s | Cell Updates / s  |
| --- | ------------------ | --------- | ----------- | -------- | --------- | ----------------- |
| 5   | [CPU] SIR          | 4096x4096 | 1,000       | 1.21     | 824.40    | 13,831,136,521.60 |
| 6   | [GPU] SIR          | 4096x4096 | 100,000     | 28.91    | 3,458.86  | 58,030,030,537.62 |
| 7   | [CPU] Game of Life | 4096x4096 | 1,000       | 1.05     | 956.43    | 16,046,254,532.72 |
| 8   | [GPU] Game of Life | 8192x8192 | 100,000     | 1.61     | 62,158.02 | ~4.17 Trillion    |

_Parameters Used_

1. `{"evaporation": "0.999", "momentum": "0.8", "num_agents": "1000", "random_action": "0.1", "reward": "1", "update_cutdown": "0.9", "world_height": "141.421", "world_width": "141.421"}`
2. `{"evaporation": "0.999", "momentum": "0.8", "num_agents": "1000000", "random_action": "0.1", "reward": "1", "update_cutdown": "0.9", "world_height": "4472.14", "world_width": "4472.14"}`
3. `{"alignment": "0.05", "cohesion": "0.0005", "max_speed": "15", "min_speed": "3", "num_agents": "1000", "protected_range": "8", "separation": "0.05", "visual_range": "50", "world_height": "141.421", "world_width": "141.421"}`
4. `{"alignment": "0.05", "cohesion": "0.0005", "max_speed": "15", "min_speed": "3", "num_agents": "50000", "protected_range": "8", "separation": "0.05", "visual_range": "50", "world_height": "1000", "world_width": "1000"}`
5. `{"grid_height": "4096", "grid_width": "4096", "infection_rate": "0.3", "initial_infected_pct": "0.01", "recovery_rate": "0.05"}`
6. `{"grid_height": "8192", "grid_width": "8192", "infection_rate": "0.3", "initial_infected_pct": "0.01", "recovery_rate": "0.05"}`
7. `{"density": "0.3", "grid_height": "4096", "grid_width": "4096"}`
8. `{"density": "0.3", "grid_height": "8192", "grid_width": "8192"}`

## Running Henad

Use any device with a CPU and optionally a GPU, and any OS that can build [wgpu](https://github.com/gfx-rs/wgpu).

### Native

Then, clone the repository and run:

```bash
cargo run --release --bin henad-app
```

Or if you wish to run in headless mode:

```bash
cargo run --release --bin henad-cli
```

### In a browser

The web build runs the same models on the same backends.

```bash
./scripts/build_web.sh serve --release   # http://localhost:8080
./scripts/build_web.sh build --release   # writes dist/
```

Use the build script rather than `trunk` directly.

The script requires `rustup toolchain install nightly --component rust-src --target wasm32-unknown-unknown`.

## Documentation

Full documentation is at https://docs.henad.micfong.space, covering installation, the models that ship with the engine, and how to write your own.
It is built from `docs/` with [Zensical](https://zensical.org):

```bash
uv run zensical serve
```

## License

Henad is licensed under [MIT](LICENSE-MIT) OR [Apache-2.0](LICENSE-APACHE), at your option. Bevy has an [excellent explanation](https://github.com/bevyengine/bevy/issues/2373) of what this means. Compiled distributions can additionally include third-party dependencies under their own terms; see [license.html](license.html) for more information.

## Contributing

Contributing is very welcome! However Henad is in very early stages so expect everything to change. It would be best if you could create an issue first to discuss what you want to contribute before making a PR.

_AI Usage Disclaimer and Policy_

This project is assisted by AI, though every line of code generated has been reviewed (and almost always, edited) by a human.

Models used include:
- Claude Code with `claude-opus-4-6`, `claude-opus-4-7`, `claude-opus-4-8`, `claude-opus-5`
- OpenCode with `gpt-5.6-terra`
- OpenCode with `grok-4.5`, `grok-4.6`

We recognise that LLM-assisted coding is evolving increasingly rapidly, but the quality of the code generated is not always guaranteed. In general, we support the [LLVM AI Tool Use Policy](https://llvm.org/docs/AIToolPolicy.html) for coding, but discourage the use of AI for communication (except for translation purposes).

All agent coding sessions since 2026-08-12 are auto-documented in `docs/developing/agent-record`, along with human comments.

And yes, this README was written entirely by a human with the help of the good-old **spellchecker** only.
