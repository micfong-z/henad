//! The command line over this crate's models, native only.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::process::ExitCode {
    let models = my_model::models().expect("model ids are unique");
    let options = henad::cli::CliOptions::new(models, henad::build_info!())
        .command_name("my-model-cli")
        .about("Command-line benchmark and sweep runner for My Model.");
    std::process::ExitCode::from(henad::cli::run(options, std::env::args_os()))
}

#[cfg(target_arch = "wasm32")]
fn main() {}
