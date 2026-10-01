# henad-build

[Henad](https://github.com/micfong-z/henad) is a parallel agent-based modelling engine, built to run millions of agents at interactive speeds on one machine.

This crate runs from a model crate's build script.
`ShaderBuild` finds the crate's WGSL shaders, composes each with the shared modules a shader reaches through `#import henad::<module>`, and writes the Rust bindings that `henad::include_shaders!` brings into the crate.

```rust,no_run
// build.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    henad_build::ShaderBuild::discover("src")?.generate()?;
    Ok(())
}
```

Use the same 0.x of `henad` and `henad-build`.
The [shaders guide](https://micfong-z.github.io/henad/authoring/shaders/) explains the naming rules and the imports.

## License

Licensed under [MIT](https://github.com/micfong-z/henad/blob/master/LICENSE-MIT) or [Apache-2.0](https://github.com/micfong-z/henad/blob/master/LICENSE-APACHE), at your option.
