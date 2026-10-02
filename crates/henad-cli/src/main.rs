//! The official `henad-cli` binary, the command line of [`henad_cli::run`] over the example models.

fn main() -> std::process::ExitCode {
    let options = henad_cli::CliOptions::new(henad_models::example_models(), henad_core::build_info!());
    std::process::ExitCode::from(henad_cli::run(options, std::env::args_os()))
}
