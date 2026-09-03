---
title: Introduction
description: Henad is a very fast agent-based modelling engine for very large populations, written in Rust.
icon: material/information-box-outline
---

# Henad

Henad is a very fast [agent-based modelling](https://en.wikipedia.org/wiki/Agent-based_model) engine for very large populations, written in [Rust](https://rust-lang.org/).
Some major highlights are:

- It is easy to use, and easy to write models that run fast.
- It can run models on the CPU or on the GPU.
- It can run the same model both natively and in a web browser without changing any code.
- It is very fast, and parallelized by default (see [Benchmarks](benchmarks.md) for details).
- It allows (many) models that previously require expensive compute to run on a laptop, and models that previously require a laptop to run on a phone.

[Try Henad in your browser](https://henad.micfong.space){ .md-button .md-button--primary }
[Get started](guide/installation.md){ .md-button }

!!! warning "Early development"

    Henad is still in early development.
    Expect bugs, missing features, and breaking changes.

## Model at a glance

The core of the Game of Life model looks like this:

```rust
--8<-- "crates/henad-models/src/game_of_life.rs:step_cell"
```

## Quick links

<div class="grid cards" markdown>

-   **[Installation](guide/installation.md)**
    
    What you need to build Henad on your machine, natively and for the web.

-   **[Running Henad](guide/running.md)**
    
    The desktop app, the browser build, and the headless benchmark runner.

-   **[The models](reference/models.md)**
    
    Eight models ship with the engine, four on the CPU and four on the GPU.

-   **[Authoring](authoring/index.md)**
  
    The four traits a model implements, and how to pick between them.

-   **[Benchmarks](benchmarks.md)**
  
    What the shipped models reach, on what hardware, measured how.

-   **[Architecture](developing/architecture.md)**
  
    How the five crates fit together and where a tick actually runs.

</div>

## Roadmap

Our [roadmap](https://github.com/users/micfong-z/projects/2) is hosted on GitHub as a project.
You can see what is being worked on, what is planned, and what is done.
You can also submit bug reports or feature requests via [issues](https://github.com/micfong-z/henad/issues).

## Licence

Henad is licensed under [MIT](https://github.com/micfong-z/henad/blob/master/LICENSE-MIT) or
[Apache-2.0](https://github.com/micfong-z/henad/blob/master/LICENSE-APACHE), at your option.
