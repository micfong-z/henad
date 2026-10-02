# henad-app

[Henad](https://github.com/micfong-z/henad) is a parallel agent-based modelling engine, built to run millions of agents at interactive speeds on one machine.

This crate is the desktop and web app.
It runs a model live in a viewport, with its parameters, statistics, charts, sweeps and results in dockable tabs.
The web build runs at [henad.micfong.space](https://henad.micfong.space).

The [app guide](https://micfong-z.github.io/henad/guide/app/) walks through each tab.

The crate is also a library.
`henad_app::run_native` and, in a browser, `henad_app::start_web` open the same app over a project's own models, and the `henad-app` binary calls them with Henad's example models.
A project that hosts the app turns off the default `example-models` feature, which brings in the example models and builds the binary.

The app embeds four fonts, listed with their sources and licences in [`assets/fonts/SOURCES.md`](https://github.com/micfong-z/henad/blob/master/crates/henad-app/assets/fonts/SOURCES.md).

## License

Licensed under [MIT](https://github.com/micfong-z/henad/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/micfong-z/henad/blob/master/LICENSE-APACHE), at your option.
