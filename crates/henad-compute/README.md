# henad-compute

[Henad](https://github.com/micfong-z/henad) is a parallel agent-based modelling engine, built to run millions of agents at interactive speeds on one machine.

This crate turns a model written against [henad-core](https://crates.io/crates/henad-core) into something that runs.
It holds the CPU engines, which step a model's struct-of-arrays lanes in parallel with rayon, and the GPU engines, which run a model's WGSL compute shaders through wgpu.
It also holds the sim threads that step a model off the UI thread, and the snapshots they publish.

The [developer documentation](https://micfong-z.github.io/henad/developing/architecture/) describes both backends.

## License

Licensed under [MIT](https://github.com/micfong-z/henad/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/micfong-z/henad/blob/master/LICENSE-APACHE), at your option.
