//! The app in a browser: the worker pool, the canvas, the prompt before a page with unsaved edits closes, and the
//! console logger.

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::wasm_bindgen::JsCast as _;
use eframe::wasm_bindgen::closure::Closure;

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
                                Cross-Origin-Embedder-Policy headers. The error appears in the browser console.";

/// Note shown in the Performance tab in a build without atomics. Such a build has no thread pool.
#[cfg(not(target_feature = "atomics"))]
const THREAD_POOL_NOTE: &str = "Models run on one thread. This build has no thread support.";

/// Whether a model the Editor holds had unsaved edits at the last frame.
static UNSAVED_EDITS: AtomicBool = AtomicBool::new(false);

/// Records whether a model the Editor holds has unsaved edits. The browser asks before the page closes or reloads
/// while it does.
pub(crate) fn set_unsaved_edits(unsaved: bool) {
    UNSAVED_EDITS.store(unsaved, Ordering::Relaxed);
}

/// Adds a `beforeunload` listener to `window` that has the browser ask before the page closes or reloads while
/// [`set_unsaved_edits`] last recorded unsaved edits. The listener lives as long as the page.
fn ask_before_unload(window: &web_sys::Window) {
    let listener = Closure::<dyn Fn(web_sys::BeforeUnloadEvent)>::new(|event: web_sys::BeforeUnloadEvent| {
        if UNSAVED_EDITS.load(Ordering::Relaxed) {
            event.prevent_default();
            // Older browsers ask only once the return value is set. None of them shows the text.
            event.set_return_value(henad_graph::sheet::sentence::UNSAVED_HEADING);
        }
    });
    if let Err(error) = window.add_event_listener_with_callback("beforeunload", listener.as_ref().unchecked_ref()) {
        log::warn!("the page will close without asking about unsaved edits: {error:?}");
    }
    listener.forget();
}

/// Starts the worker pool at the width that `?threads=` requests, then builds the options and starts the app on the
/// `the_canvas_id` canvas, removing the `loading_text` element.
///
/// Once the app has started, the browser asks before the page closes or reloads while a model the Editor holds has
/// unsaved edits.
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
            element.set_text_content(Some(&crate::ui::text::capitalize(&error.to_string())));
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
            ask_before_unload(&window);
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
