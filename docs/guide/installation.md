---
title: Installation
description: How to install Henad's app and command line, and how to start your own project.
icon: material/download-outline
---

# Installation

You can try Henad with its example models in [the web app](https://henad.micfong.space), with nothing to install.
To run it on your machine, install the app and the command line from crates.io, or build them from a clone of the repository.
To write your own models, start a project from the template instead.

Henad runs on any device with a CPU, optionally a GPU, and any operating system that can build [wgpu](https://github.com/gfx-rs/wgpu).

???+ info "Supported platforms"

    As per the [wgpu documentation](https://github.com/gfx-rs/wgpu#supported-platforms), Henad can be run on the following platforms as of wgpu v30:

    | API    | Windows                                                                                             | Linux/Android                                                | macOS/iOS                                               | Web (wasm)                                               |
    | ------ | --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------ | ------------------------------------------------------- | -------------------------------------------------------- |
    | Vulkan | :material-check-all:{ title="First Class Support" }                                                 | :material-check-all:{ title="First Class Support" }          | :material-volcano-outline:{ title="Requires MoltenVK" } |                                                          |
    | Metal  |                                                                                                     |                                                              | :material-check-all:{ title="First Class Support" }     |                                                          |
    | DX12   | :material-check-all:{ title="First Class Support" }                                                 |                                                              |                                                         |                                                          |
    | OpenGL | :material-check:{ title="Best Effort Support" }, or :material-set-square:{ title="Requires ANGLE" } | :material-check:{ title="Best Effort Support" } (GL ES 3.0+) | :material-set-square:{ title="Requires ANGLE" }         | :material-check:{ title="Best Effort Support" } (WebGL2) |
    | WebGPU |                                                                                                     |                                                              |                                                         | :material-check-all:{ title="First Class Support" }      |

    - :material-check-all: = First Class Support  
    - :material-check: = Downlevel/Best Effort Support  
    - :material-set-square: = Requires the [ANGLE](https://github.com/gfx-rs/wgpu/wiki/Running-on-ANGLE) translation layer (GL ES 3.0 only).
      On macOS/iOS, use the `angle` feature.
      On Windows, `gles` uses WGL by default.
      Build with `cfg(windows_angle)` to use ANGLE instead.
    - :material-volcano-outline: = Requires the [MoltenVK](https://vulkan.lunarg.com/sdk/home#mac) translation layer  

## Installing the app and the command line

With a Rust toolchain from [rustup](https://rustup.rs), install both from crates.io:

``` bash
cargo install --locked --config profile.release.opt-level=2 henad-app henad-cli
```

This puts `henad-app` and `henad-cli` on your path, with the ten example models.
`--locked` builds with the dependency versions the release was tested with.
The `--config` builds at opt-level 2, the level every Henad number is measured at.
Without it `cargo install` builds at 3.

## Building from a clone

``` bash
git clone https://github.com/micfong-z/henad.git
cd henad
```

On the first build, rustup installs the toolchain that the repository pins.
`cargo run --release --bin henad-app` and `cargo run --release -p henad-cli` then run the two binaries, as [Running Henad](running.md) shows.

The web build needs the dated nightly toolchain the repository pins, with the `rust-src` component for wasm threads.
Run this from the repository root:

``` bash
rustup toolchain install "$(cat templates/model-project/scripts/web-toolchain)" --profile minimal \
  --component rust-src,clippy --target wasm32-unknown-unknown
```

It also needs [Trunk](https://github.com/trunk-rs/trunk), at the release the repository pins:

``` bash
cargo install --locked trunk --version "$(cat templates/model-project/scripts/trunk-version)"
```

Use `scripts/build_web.sh` to build the web app.

## Starting a project

Your own models live in your own project, with its own app, command line, web build, tests and CI, on the published crates.
[Your own project](your-project.md) fetches the template and sets it up, and the [first-model tutorials](first-model/game-of-life.md) add models to it.

A Rust program that runs the example models, sweeps them or opens the app needs no template, and [Using Henad from code](library.md) shows one.

## Check the install

``` bash
henad-cli --info
```

From a clone, run `cargo run -p henad-cli -- --info` instead.
This prints the host details and the GPU adapter wgpu selected.
If an adapter line is printed, the four GPU models are available.
Otherwise, only the CPU models are available.

### Packages on Linux

A desktop distribution ships everything Henad needs.
A server or container image might lack a Vulkan loader and driver, and `henad-cli --info` then prints no adapter.
On Debian and Ubuntu, install `libvulkan1` with `mesa-vulkan-drivers` or your GPU vendor's driver.
On Fedora, install `vulkan-loader` with `mesa-vulkan-drivers`.
On X11 the app needs `libxkbcommon-x11`, packaged as `libxkbcommon-x11-0` on Debian and Ubuntu.
Its file dialogs need `xdg-desktop-portal` with a backend for your desktop, such as `xdg-desktop-portal-gtk`.

*[wasm]: WebAssembly
