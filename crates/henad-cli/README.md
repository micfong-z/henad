# henad-cli

[Henad](https://github.com/micfong-z/henad) is a parallel agent-based modelling engine, built to run millions of agents at interactive speeds on one machine.

This crate is the command-line program.
It benchmarks a model headlessly, exports its state and statistics, and runs sweeps and searches.

```bash
henad-cli --list
henad-cli boids --steps 100 --reps 3
```

The [command-line reference](https://micfong-z.github.io/henad/reference/cli/) lists every flag.

The crate is also a library.
`henad_cli::run` runs the same command line over a project's own models, and the `henad-cli` binary is three lines over it with Henad's example models.
A project that hosts the command line turns off the default `example-models` feature, which brings in the example models and builds the binary.

## License

Licensed under [MIT](https://github.com/micfong-z/henad/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/micfong-z/henad/blob/master/LICENSE-APACHE), at your option.
