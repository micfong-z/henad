---
title: Installation
description: How to install Henad on your machine.
icon: material/download-outline
---

# Installation

You can test Henad out with default models using [the web app](https://henad.micfong.space).
Alternatively, if you want to run it locally or write your own models, you can build the desktop app or the web app from source.

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
      On Windows, `gles` uses WGL by default; build with `cfg(windows_angle)` to use ANGLE instead.
    - :material-volcano-outline: = Requires the [MoltenVK](https://vulkan.lunarg.com/sdk/home#mac) translation layer  

``` bash
git clone https://github.com/micfong-z/henad.git
cd henad
```

## Rust toolchain

rustup should automatically install the correct toolchain on first build.

However, if you want to build the web app, you need to install the nightly toolchain with the `rust-src` component for wasm threads:

``` bash
rustup toolchain install nightly --component rust-src --target wasm32-unknown-unknown
```

## Web build

The web build also require [Trunk](https://github.com/trunk-rs/trunk):

``` bash
cargo install --locked trunk
```

Use `scripts/build_web.sh` to build the web app.

## Check the install

``` bash
cargo run -p henad-cli -- --info
```

This prints the host details and the GPU adapter wgpu selected.
If an adapter line is printed, it means the four GPU models are available.
Otherwise, only the CPU models are available.

*[wasm]: WebAssembly