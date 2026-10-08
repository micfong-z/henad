---
title: Running Henad
description: How to run Henad's app, web app and command line on your machine.
icon: material/play-outline
---

# Running Henad

Before running Henad, [install it](installation.md) or clone the repository.
If you just want to try out Henad with the example models, you can use [the web app](https://henad.micfong.space) instead.

## Desktop app

With the app installed, run:

``` bash
henad-app
```

From a clone of the repository, run:

``` bash
cargo run --release --bin henad-app
```

In [your own project](your-project.md), `cargo run --release` runs the project's app over its own models.

!!! warning "Release mode"

    The debug build is slow, and the sim thread will not run at full speed.
    Make sure that `--release` is used to build the release version.

See the [app tour](app.md) for an introduction to the UI.

## Web app

To build the web app from a clone, use the following command:

``` bash
./scripts/build_web.sh serve --release   # starts server at http://localhost:8080
```

Then open [http://localhost:8080](http://localhost:8080) in a browser that supports WebGPU.

Alternatively, to deploy the web app, run:

``` bash
./scripts/build_web.sh build --release   # writes to dist/
```

!!! warning "Web build performance"

    CPU models run noticeably slower in the browser.
    Run natively when you want the engine at full speed.

    Nonetheless, GPU model performance is close to native.

Since the thread pool needs `SharedArrayBuffer`, the web build serves cross-origin isolated.
Hosts deploying Henad need to send `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`.

See the [app tour](app.md) for an introduction to the UI.

## CLI

If you do not need to render anything, you can use the Henad CLI.
This is mainly for benchmarking purposes, but it can also be used if you want to run Henad in a headless environment.

Run the following command to see the available flags:

``` bash
henad-cli --help
```

From a clone, write `cargo run --release -p henad-cli --` instead of `henad-cli`, and in your own project, `cargo run --release --bin my-model-cli --`.

See [the Henad CLI reference](../reference/cli.md) for more details.
See [parameter sweeps](sweeps.md) for running a model over many parameter values and seeds at once, and [searching a model](search.md) for letting Henad pick the values.

*[CLI]: Command-line interface
*[UI]: User interface
*[CPU]: Central processing unit
*[GPU]: Graphics processing unit
