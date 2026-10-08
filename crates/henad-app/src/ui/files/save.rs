//! Save dialogs that write bytes to a file, or pass them to the browser's downloads.
//!
//! A browser offers no save dialog that reports what the user chose. The app passes each file straight to the
//! browser's downloads, which saves it or asks where to save, depending on browser settings.

use std::sync::Arc;

use henad_explore::output::memory::SweepFiles;

use super::{SaveOutcome, SaveResult, SaveTarget, spawn};

/// Opens a save dialog for `name` and writes `bytes` into the file the user picks, or downloads them in a browser.
///
/// Returns immediately. The outcome arrives on `sender`, and `ctx` repaints to show it.
pub fn spawn_save(
    target: SaveTarget,
    name: String,
    bytes: Vec<u8>,
    sender: flume::Sender<SaveOutcome>,
    ctx: egui::Context,
) {
    spawn(async move {
        let result = save_file(&name, &bytes).await;
        report(&sender, &ctx, SaveOutcome { target, result });
    });
}

/// Saves the four files of a sweep, `files`, together, sharing their bytes with the caller.
///
/// On native the user picks a folder, and every file is written into it. A folder that holds a file of the same name
/// is rejected. A browser downloads the files one after another.
///
/// Returns immediately. The outcome arrives on `sender`, and `ctx` repaints to show it.
pub fn spawn_save_files(
    target: SaveTarget,
    files: Arc<SweepFiles>,
    sender: flume::Sender<SaveOutcome>,
    ctx: egui::Context,
) {
    spawn(async move {
        let result = save_files(&files.entries()).await;
        report(&sender, &ctx, SaveOutcome { target, result });
    });
}

/// Sends `outcome` and repaints, so an idle app shows it without waiting for input.
fn report(sender: &flume::Sender<SaveOutcome>, ctx: &egui::Context, outcome: SaveOutcome) {
    // The receiver is gone once the app is closing, and nothing is left to report to.
    drop(sender.send(outcome));
    ctx.request_repaint();
}

#[cfg(not(target_arch = "wasm32"))]
async fn save_file(name: &str, bytes: &[u8]) -> SaveResult {
    match rfd::AsyncFileDialog::new().set_file_name(name).save_file().await {
        Some(handle) => match handle.write(bytes).await {
            Ok(()) => SaveResult::Saved(handle.file_name()),
            Err(error) => SaveResult::Failed(error.to_string()),
        },
        None => SaveResult::Canceled,
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn save_files(files: &[(&str, &[u8])]) -> SaveResult {
    let folder_handle = rfd::AsyncFileDialog::new()
        .set_title("Select folder")
        .set_can_create_directories(true)
        .pick_folder()
        .await;
    let Some(folder) = folder_handle else {
        return SaveResult::Canceled;
    };
    write_into(folder.path(), files)
}

/// Writes every file of `files` into `folder`, rejecting a folder that holds an entry of the same name.
///
/// A link counts as an entry, a dangling link included. Otherwise the write would follow the link out of `folder`.
#[cfg(not(target_arch = "wasm32"))]
fn write_into(folder: &std::path::Path, files: &[(&str, &[u8])]) -> SaveResult {
    if let Some((name, _)) = files
        .iter()
        .find(|(name, _)| folder.join(name).symlink_metadata().is_ok())
    {
        return SaveResult::Failed(format!("{} already contains a file named {name}", folder.display()));
    }
    for (name, bytes) in files {
        if let Err(error) = std::fs::write(folder.join(name), bytes) {
            return SaveResult::Failed(format!("{name}: {error}"));
        }
    }
    SaveResult::Saved(format!("{} files to {}", files.len(), folder.display()))
}

#[cfg(target_arch = "wasm32")]
async fn save_file(name: &str, bytes: &[u8]) -> SaveResult {
    match download(name, bytes) {
        Ok(()) => SaveResult::Downloaded(name.to_owned()),
        Err(message) => SaveResult::Failed(message),
    }
}

#[cfg(target_arch = "wasm32")]
async fn save_files(files: &[(&str, &[u8])]) -> SaveResult {
    match files.iter().try_for_each(|(name, bytes)| download(name, bytes)) {
        Ok(()) => SaveResult::DownloadsStarted,
        Err(message) => SaveResult::Failed(message),
    }
}

/// Time in milliseconds a download's object URL stays valid. A browser that asks where to save reads the bytes
/// before it asks.
#[cfg(target_arch = "wasm32")]
const DOWNLOAD_URL_LIFETIME_MS: i32 = 60_000;

/// Passes `bytes` to the browser's downloads under the file name `name`.
///
/// # Errors
///
/// Returns the browser's error, written out, when the page has no document or rejects the download.
#[cfg(target_arch = "wasm32")]
fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
    use eframe::wasm_bindgen::JsCast as _;
    use eframe::wasm_bindgen::JsValue;
    use eframe::wasm_bindgen::closure::Closure;

    let describe = |error: JsValue| format!("the browser refused the download of {name}: {error:?}");
    let window = web_sys::window().ok_or_else(|| "the app runs outside a browser window".to_owned())?;
    let document = window.document().ok_or_else(|| "the page has no document".to_owned())?;
    let body = document.body().ok_or_else(|| "the page has no body".to_owned())?;

    // A copy out of the wasm memory. A threaded build shares that memory between workers, and a blob rejects a view
    // of shared memory.
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/octet-stream");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(describe)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(describe)?;

    let anchor = document
        .create_element("a")
        .map_err(describe)?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|element| describe(element.into()))?;
    anchor.set_href(&url);
    anchor.set_download(name);
    body.append_child(&anchor).map_err(describe)?;
    anchor.click();
    anchor.remove();

    let revoke = Closure::once_into_js(move || drop(web_sys::Url::revoke_object_url(&url)));
    window
        .set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), DOWNLOAD_URL_LIFETIME_MS)
        .map_err(describe)?;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::PathBuf;

    use super::{SaveResult, write_into};

    /// Folder removed with its contents when dropped.
    struct ScratchFolder(PathBuf);

    impl Drop for ScratchFolder {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    #[test]
    fn a_link_to_nothing_with_a_file_name_refuses_the_folder() {
        let scratch = ScratchFolder(std::env::temp_dir().join(format!("henad-app-save-link-{}", std::process::id())));
        let folder = scratch.0.join("picked");
        std::fs::create_dir_all(&folder).expect("the scratch folder is created");
        let target = scratch.0.join("outside.csv");
        std::os::unix::fs::symlink(&target, folder.join("runs.csv")).expect("the link is created");
        let result = write_into(&folder, &[("runs.csv", b"run_id\n")]);
        assert!(matches!(result, SaveResult::Failed(_)));
        assert!(!target.exists(), "nothing is written through the link");
    }
}
