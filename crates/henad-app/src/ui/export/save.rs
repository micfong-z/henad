//! Handing bytes to a save dialog, on whichever target the app is running.
//!
//! `rfd` is async and the app has no executor, so each target drives the future with what it
//! already has. Native blocks a thread of its own. On the web the dialog returns without
//! prompting, and the browser asks where to put the file when `write` runs.

/// What became of one save, reported back to the Export panel.
pub enum SaveOutcome {
    Saved(String),
    Failed(String),
    /// The user dismissed the dialog.
    Cancelled,
}

/// Open a save dialog for `name` and write `bytes` into whatever the user picks.
///
/// Returns immediately. The outcome arrives on `tx`.
pub fn spawn_save(name: String, bytes: Vec<u8>, tx: flume::Sender<SaveOutcome>) {
    let task = async move {
        let outcome = match rfd::AsyncFileDialog::new().set_file_name(&name).save_file().await {
            Some(handle) => match handle.write(&bytes).await {
                Ok(()) => SaveOutcome::Saved(handle.file_name()),
                Err(err) => SaveOutcome::Failed(err.to_string()),
            },
            None => SaveOutcome::Cancelled,
        };
        // The receiver is gone once the app is closing, and nothing is left to report to.
        drop(tx.send(outcome));
    };
    spawn(task);
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn(task: impl Future<Output = ()> + Send + 'static) {
    std::thread::spawn(move || pollster::block_on(task));
}

/// Nothing to spawn onto in a browser, and no blocking allowed on the main thread either.
#[cfg(target_arch = "wasm32")]
fn spawn(task: impl Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}
