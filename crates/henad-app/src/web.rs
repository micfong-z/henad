//! The app in a browser: the worker pool, the canvas and the console logger.

use eframe::wasm_bindgen::JsCast as _;

use crate::HenadApp;
use crate::init::wgpu_configuration;
use crate::options::{AppOptions, WebStartError};

/// Id of the canvas the app draws on.
const CANVAS_ID: &str = "the_canvas_id";

/// Id of the element that shows the page's loading text, removed once the app has started.
const LOADING_TEXT_ID: &str = "loading_text";

/// Note shown in the Performance tab when the thread pool failed to start.
#[cfg(target_feature = "atomics")]
const THREAD_POOL_NOTE: &str = "Models run on one thread. The page might lack the Cross-Origin-Opener-Policy and \
                                Cross-Origin-Embedder-Policy headers, and the browser console has the error.";

/// Note shown in the Performance tab in a build without atomics. Such a build has no thread pool.
#[cfg(not(target_feature = "atomics"))]
const THREAD_POOL_NOTE: &str = "Models run on one thread. This build has no thread support.";

/// Starts the worker pool at the width that `?threads=` requests, then builds the options and starts the app on the
/// `the_canvas_id` canvas, removing the `loading_text` element.
///
/// A pool that fails to start is logged, and the models then run on one thread, with a note in the Performance tab.
/// A build without atomics has no pool, and runs the models on one thread with its own note.
/// A failure to start the app is returned, for the caller to log, and written into the `loading_text` element where
/// the page has one.
///
/// Note that nothing before this call may touch rayon. A rayon call there builds a one-thread pool, and the worker pool
/// then cannot start.
///
/// # Errors
///
/// Returns an error outside a browser window with a document, and when the options' models cannot serve their
/// opening, the page has no canvas `the_canvas_id`, or eframe fails to start.
pub async fn start_web(options: impl FnOnce() -> AppOptions) -> Result<(), WebStartError> {
    let Some((window, document)) = web_sys::window().and_then(|window| {
        let document = window.document()?;
        Some((window, document))
    }) else {
        return Err(WebStartError::no_window());
    };

    let pool_failed = !start_thread_pool(&window).await;

    let mut options = options();
    if pool_failed {
        options.thread_pool_note = Some(THREAD_POOL_NOTE.to_owned());
    }
    let loading_text = document.get_element_by_id(LOADING_TEXT_ID);
    let refuse = |error: WebStartError| {
        if let Some(element) = &loading_text {
            element.set_text_content(Some(&crate::ui::sweep::draft::capitalize(&error.to_string())));
        }
        Err(error)
    };

    if let Err(error) = options.check_opening() {
        return refuse(WebStartError::opening(error));
    }
    let Some(canvas) = document
        .get_element_by_id(CANVAS_ID)
        .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
    else {
        return refuse(WebStartError::missing_canvas(CANVAS_ID));
    };

    let web_options = eframe::WebOptions {
        wgpu_options: wgpu_configuration(options.models.gpu_needs()),
        ..Default::default()
    };
    let started = eframe::WebRunner::new()
        .start(
            canvas,
            web_options,
            Box::new(|cc| Ok(Box::new(HenadApp::new(cc, options)))),
        )
        .await;
    match started {
        Ok(()) => {
            if let Some(element) = &loading_text {
                element.remove();
            }
            Ok(())
        }
        Err(error) => refuse(WebStartError::eframe(&error)),
    }
}

/// Starts the worker pool at the width that `?threads=` requests, and returns whether it started.
#[cfg(target_feature = "atomics")]
async fn start_thread_pool(window: &web_sys::Window) -> bool {
    let available = window.navigator().hardware_concurrency() as usize;
    let search = window.location().search().unwrap_or_default();
    let workers = crate::requested_threads(&search, available);
    log::info!("thread pool: {workers} workers, {available} reported by the browser");
    wasm_bindgen_futures::JsFuture::from(crate::init_thread_pool(workers))
        .await
        .inspect_err(|error| log::error!("thread pool init failed, models will run on one core: {error:?}"))
        .is_ok()
}

/// Returns `false`. A build without atomics has no thread pool, and rayon runs every task on the calling thread.
#[cfg(not(target_feature = "atomics"))]
async fn start_thread_pool(_window: &web_sys::Window) -> bool {
    log::warn!("this build has no atomics, models will run on one core");
    false
}

/// Installs a logger that writes to the browser console, for messages at `level` and above.
pub fn init_web_logger(level: log::LevelFilter) {
    eframe::WebLogger::init(level).ok();
}
