# henad-explore

[Henad](https://github.com/micfong-z/henad) is a parallel agent-based modelling engine, built to run millions of agents at interactive speeds on one machine.

This crate runs parameter sweeps and searches over a model, and writes their results to a folder of CSV tables with a manifest.
Sweeps cover factorial, zip, Latin hypercube, random and table designs, and searches cover random search, hill climbing, a genetic algorithm and Pattern Space Exploration.
A sweep can be sharded across machines, merged and resumed.

The [sweeps guide](https://micfong-z.github.io/henad/guide/sweeps/) and the [search guide](https://micfong-z.github.io/henad/guide/search/) describe both.

## License

Licensed under [MIT](https://github.com/micfong-z/henad/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/micfong-z/henad/blob/master/LICENSE-APACHE), at your option.
