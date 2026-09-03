---
title: Running Henad
description: How to run Henad on your machine.
icon: material/play-outline
---

# Running Henad

Before running Henad, you need to [install Henad](installation.md) first.
If you just want to try out Henad with the default models, you can use [the web app](https://henad.micfong.space) instead.

## Desktop app

To run the desktop app, use the following command:

``` bash
cargo run --release --bin henad-app
```

!!! warning "Release mode"

    The debug build is slow, and the sim thread will not run at full speed.
    Make sure that `--release` is used to build the release version.

See [app tour](app.md) for an introduction of the UI.

## Web app

To build the web app, use the following command:

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

    Nonetheless, GPU model performance are close to native.

Since the thread pool needs `SharedArrayBuffer`, the web build serves cross-origin isolated.
Hosts deploying Henad need to send `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`.

See [app tour](app.md) for an introduction of the UI.

## CLI

If you do not need to render anything, you can use Henad using CLI.
This is mainly for benchmarking purposes, but it can also be used if you want to run Henad in a headless environment.

Run the following command to see the available flags:

``` bash
cargo run --release -p henad-cli -- --help
```

See [the Henad CLI reference](../reference/cli.md) for more details.

*[CLI]: Command-line interface
*[UI]: User interface
*[CPU]: Central processing unit
*[GPU]: Graphics processing unit