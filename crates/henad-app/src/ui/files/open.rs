//! Opens that read the files, or take the folder, the user picks.

use super::{DialogFile, OpenOutcome, OpenResult, OpenTarget, spawn};

/// Opens a dialog for `target`, and reads the files or takes the folder the user picks.
///
/// Returns immediately. The outcome arrives on `sender`.
pub fn spawn_open(target: OpenTarget, sender: flume::Sender<OpenOutcome>) {
    spawn(async move {
        let result = pick(target).await;
        // The receiver is gone once the app is closing, and nothing is left to report to.
        drop(sender.send(OpenOutcome { target, result }));
    });
}

async fn pick(target: OpenTarget) -> OpenResult {
    match target {
        OpenTarget::SweepSpec => pick_file("Sweep spec", &["toml"]).await,
        OpenTarget::DesignTable => pick_file("Design table", &["csv"]).await,
        OpenTarget::OutputFolder => pick_folder().await,
        OpenTarget::Results => pick_results().await,
    }
}

async fn pick_file(filter: &str, extensions: &[&str]) -> OpenResult {
    let Some(handle) = rfd::AsyncFileDialog::new()
        .add_filter(filter, extensions)
        .pick_file()
        .await
    else {
        return OpenResult::Canceled;
    };
    match read(handle).await {
        Ok(file) => OpenResult::Files(vec![file]),
        Err(message) => OpenResult::Failed(message),
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn read(handle: rfd::FileHandle) -> Result<DialogFile, String> {
    let path = handle.path().to_owned();
    let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(DialogFile {
        name: handle.file_name(),
        bytes,
        path: Some(path),
    })
}

#[cfg(target_arch = "wasm32")]
async fn read(handle: rfd::FileHandle) -> Result<DialogFile, String> {
    Ok(DialogFile {
        name: handle.file_name(),
        bytes: handle.read().await,
        path: None,
    })
}

#[cfg(not(target_arch = "wasm32"))]
async fn pick_folder() -> OpenResult {
    match rfd::AsyncFileDialog::new()
        .set_can_create_directories(true)
        .pick_folder()
        .await
    {
        Some(handle) => OpenResult::Folder(handle.path().to_owned()),
        None => OpenResult::Canceled,
    }
}

#[cfg(target_arch = "wasm32")]
async fn pick_folder() -> OpenResult {
    OpenResult::Failed("Use the desktop app to select a folder".to_owned())
}

#[cfg(not(target_arch = "wasm32"))]
async fn pick_results() -> OpenResult {
    pick_folder().await
}

/// A browser cannot hand over a folder, so the user picks the manifest and the CSV files inside one.
#[cfg(target_arch = "wasm32")]
async fn pick_results() -> OpenResult {
    let Some(handles) = rfd::AsyncFileDialog::new()
        .add_filter("Sweep results", &["json", "csv"])
        .pick_files()
        .await
    else {
        return OpenResult::Canceled;
    };
    let mut files = Vec::with_capacity(handles.len());
    for handle in handles {
        match read(handle).await {
            Ok(file) => files.push(file),
            Err(message) => return OpenResult::Failed(message),
        }
    }
    OpenResult::Files(files)
}
