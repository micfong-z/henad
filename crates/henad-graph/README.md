# henad-graph

[Henad](https://github.com/micfong-z/henad) is a parallel agent-based modelling engine, built to run millions of agents at interactive speeds on one machine.

This crate holds graph models, the models a researcher builds in the node editor without writing code.
A graph model is a TOML model file of kind `graph`, and the crate reads it with a place in the file for every table and key, and writes it in one canonical form.
The canonical text, without the editor's layout, is the model's code identity in a results folder.
The crate checks a graph and reports every problem at once, runs its init passes on the CPU with the host evaluator, and compiles a graph on a grid or of agents to WGSL with the model entry that runs it on the GPU.
Graph starters cover Game of Life, Vote and an SIR epidemic on a grid, and boids, ants and a random walk of agents.

The crate also holds sheets, the author documents a researcher writes as numbered sentences on cards.
A sheet is a model file of kind `sheet`, and the crate reads, checks and writes it, edits it, and lowers it to the graph model it runs as.
Sheet starters cover Game of Life, an SIR epidemic, boids, ants and other models.

The [Editor guide](https://micfong-z.github.io/henad/guide/editor/) walks through building a model in the app, and the [model file reference](https://micfong-z.github.io/henad/reference/model-files/) describes each kind of model file.

## License

Licensed under [MIT](https://github.com/micfong-z/henad/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/micfong-z/henad/blob/master/LICENSE-APACHE), at your option.
