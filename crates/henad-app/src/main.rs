#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    use eframe::egui_wgpu;

    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).

    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(egui_wgpu::WgpuSetupCreateNew {
                device_descriptor: std::sync::Arc::new(|adapter| {
                    let base = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
                    let mut descriptor = (base.device_descriptor)(adapter);
                    // Enables the GPU per-step timing readout for GPU-backed models, when the
                    // adapter supports it; falls back to reporting "N/A" otherwise.
                    if adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
                        descriptor.required_features |= wgpu::Features::TIMESTAMP_QUERY;
                    }
                    descriptor
                }),
                ..egui_wgpu::WgpuSetupCreateNew::without_display_handle()
            }),
            ..Default::default()
        },
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_min_inner_size([300.0, 220.0])
            .with_icon(
                // NOTE: Adding an icon is optional
                eframe::icon_data::from_png_bytes(&include_bytes!("../../../assets/icon-256.png")[..])
                    .expect("Failed to load icon"),
            ),
        ..Default::default()
    };
    eframe::run_native(
        "Henad Engine",
        native_options,
        Box::new(|cc| Ok(Box::new(henad_app::HenadApp::new(cc)))),
    )
}

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    // Redirect `log` message to `console.log` and friends:
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window().expect("No window").document().expect("No document");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(henad_app::HenadApp::new(cc)))),
            )
            .await;

        // Remove the loading text and spinner:
        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(_) => {
                    loading_text.remove();
                }
                Err(e) => {
                    loading_text.set_inner_html("<p> The app has crashed. See the developer console for details. </p>");
                    panic!("Failed to start eframe: {e:?}");
                }
            }
        }
    });
}
