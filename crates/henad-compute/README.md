# henad-compute

[Henad](https://github.com/micfong-z/henad) is a parallel agent-based modelling engine, built to run millions of agents at interactive speeds on one machine.

This crate turns a model written against [henad-core](https://crates.io/crates/henad-core) into something that runs.
It holds the CPU engines, which step a model's struct-of-arrays lanes in parallel with rayon, and the GPU engines, which run a model's WGSL compute shaders through wgpu.
It also holds the sim threads that step a model off the UI thread, and the snapshots they publish.
A model is registered as an entry that a host can list and build.
A program builds an entry with checked values into a `Simulation` and steps it.
With the `runtime-wgsl` feature it also composes WGSL while a program runs and builds grid rules from it.
The same feature makes public the GPU model specs, such as `GpuGridSpec` and `GpuAgentSpec`, and the `ModelEntry` constructors that build an entry from a spec.

The [developer documentation](https://micfong-z.github.io/henad/developing/architecture/) describes both backends.

## License

Licensed under [MIT](https://github.com/micfong-z/henad/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/micfong-z/henad/blob/master/LICENSE-APACHE), at your option.
