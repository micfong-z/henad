//! The app over this crate's models, natively and on the web.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use henad::app::AppOptions;

fn options() -> AppOptions {
    let models = my_model::models().expect("model ids are unique");
    AppOptions::new(models, "My Model", henad::build_info!())
        // The icon sits outside `src` and changes no result. A file a model reads at compile time goes under `src`.
        .icon_png(include_bytes!("../assets/icon-256.png"))
        .cli_command("my-model-cli")
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), henad::app::AppError> {
    env_logger::init();
    let mut options = options();
    if let Some(folder) = henad::app::results_folder(std::env::args_os().skip(1)) {
        options = options.opening(henad::app::AppOpening::Results(folder));
    }
    henad::app::run_native(options)
}

#[cfg(target_arch = "wasm32")]
fn main() {
    henad::app::init_web_logger(log::LevelFilter::Info);
    wasm_bindgen_futures::spawn_local(async {
        if let Err(error) = henad::app::start_web(options).await {
            log::error!("the app did not start: {error}");
        }
    });
}
