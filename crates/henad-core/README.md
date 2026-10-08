# henad-core

[Henad](https://github.com/micfong-z/henad) is a parallel agent-based modelling engine, built to run millions of agents at interactive speeds on one machine.

This crate holds what a model is written against: the model traits for grids, agent populations and networks on the CPU and the GPU, the primitives their kernels call, the parameter, statistic and action descriptors, and the sweep and search planning that needs no engine.
It has no dependencies.

The [authoring guide](https://micfong-z.github.io/henad/authoring/) explains each trait, and [Writing your first model](https://micfong-z.github.io/henad/guide/first-model/game-of-life/) builds one step by step.

## License

Licensed under [MIT](https://github.com/micfong-z/henad/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/micfong-z/henad/blob/master/LICENSE-APACHE), at your option.
