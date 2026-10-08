// Hides the console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use henad_app::AppOptions;

/// Returns the official app: the example models under Henad's name, icon, links and licence.
fn options() -> AppOptions {
    AppOptions::new(henad_models::example_models(), "Henad", henad_core::build_info!())
        .icon_png(include_bytes!("../assets/icon-256.png"))
        .source_url("https://github.com/micfong-z/henad")
        .documentation_url("https://micfong-z.github.io/henad/")
        .license(env!("CARGO_PKG_LICENSE"))
        .cli_command("henad-cli")
        .__official()
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), henad_app::AppError> {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
    let mut options = options();
    if let Some(folder) = henad_app::results_folder(std::env::args_os().skip(1)) {
        options = options.opening(henad_app::AppOpening::Results(folder));
    }
    henad_app::run_native(options)
}

#[cfg(target_arch = "wasm32")]
fn main() {
    henad_app::init_web_logger(log::LevelFilter::Debug);
    wasm_bindgen_futures::spawn_local(async {
        if let Err(error) = henad_app::start_web(options).await {
            log::error!("the app did not start: {error}");
        }
    });
}
