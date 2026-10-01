//! Saves that hand bytes to a save dialog.
//!
//! On the web the dialog returns without prompting, and the browser asks where to put the file when `write` runs.

use std::sync::Arc;

use henad_explore::output::memory::SweepFiles;

use super::{SaveOutcome, SaveResult, SaveTarget, spawn};

/// Opens a save dialog for `name` and writes `bytes` into the file the user picks.
///
/// Returns immediately. The outcome arrives on `sender`.
pub fn spawn_save(target: SaveTarget, name: String, bytes: Vec<u8>, sender: flume::Sender<SaveOutcome>) {
    spawn(async move {
        let result = save_file(&name, &bytes).await;
        // The receiver is gone once the app is closing, and nothing is left to report to.
        drop(sender.send(SaveOutcome { target, result }));
    });
}

/// Saves the four files of a sweep, `files`, together, sharing their bytes with the caller.
///
/// Native asks for a folder and writes every file into it, refusing a folder that holds a file of the same name. A
/// browser downloads the files one after another. Returns immediately. The outcome arrives on `sender`.
pub fn spawn_save_files(target: SaveTarget, files: Arc<SweepFiles>, sender: flume::Sender<SaveOutcome>) {
    spawn(async move {
        let result = save_files(&files.entries()).await;
        drop(sender.send(SaveOutcome { target, result }));
    });
}

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
    let folder = folder.path();
    if let Some((name, _)) = files.iter().find(|(name, _)| folder.join(name).exists()) {
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
async fn save_files(files: &[(&str, &[u8])]) -> SaveResult {
    for (name, bytes) in files {
        let result = save_file(name, bytes).await;
        if !matches!(result, SaveResult::Saved(_)) {
            return result;
        }
    }
    SaveResult::Saved(format!("{} files", files.len()))
}
