//! The app's window on a desktop, and the results folder a command line names.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::HenadApp;
use crate::init::wgpu_configuration;
use crate::options::{AppError, AppOptions};

/// Opens the app window and returns when it closes.
///
/// # Errors
///
/// Returns an error before the window opens when the options' models cannot serve their opening, and when eframe
/// cannot open the window or fails while it is open.
///
/// # Panics
///
/// Panics when called off the main thread. winit refuses to start an event loop on any other thread.
pub fn run_native(options: AppOptions) -> Result<(), AppError> {
    options.check_opening().map_err(AppError::opening)?;

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([400.0, 300.0])
        .with_min_inner_size([300.0, 220.0]);
    match eframe::icon_data::from_png_bytes(options.product.icon_png) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(error) => log::warn!("the app icon is no PNG and is left out: {error}"),
    }
    if let Some(storage) = storage_id(&options.product.name) {
        viewport = viewport.with_app_id(storage);
    }
    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: wgpu_configuration(options.models.gpu_needs()),
        viewport,
        ..Default::default()
    };
    let name = options.product.name.clone();
    eframe::run_native(
        &name,
        native_options,
        Box::new(|cc| Ok(Box::new(HenadApp::new(cc, options)))),
    )
    .map_err(AppError::eframe)
}

/// Folder name eframe stores the app's state under for a product name with nothing left once [`storage_id`] cleans
/// it.
const FALLBACK_STORAGE_ID: &str = "henad-app";

/// Returns the folder name eframe stores the state of product `name` under, `None` when `name` can serve as it is.
///
/// Each character a folder name cannot hold on some platform becomes `-`, and spaces and dots at either end go. A
/// name with nothing left stores under [`FALLBACK_STORAGE_ID`].
fn storage_id(name: &str) -> Option<String> {
    let replaced: String = name
        .chars()
        .map(|character| {
            if character.is_control() || matches!(character, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                character
            }
        })
        .collect();
    let trimmed = replaced.trim_matches(|character: char| character == '.' || character.is_whitespace());
    let id = if trimmed.is_empty() {
        FALLBACK_STORAGE_ID
    } else {
        trimmed
    };
    (id != name).then(|| id.to_owned())
}

/// Returns the folder `--open DIR` or `--open=DIR` names among `arguments`, the command line without the program.
///
/// An argument starting with `--` is a flag and never the folder after `--open`.
pub fn results_folder(arguments: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let mut arguments = arguments.into_iter().peekable();
    while let Some(argument) = arguments.next() {
        if argument == "--open" {
            if let Some(folder) = arguments.next_if(|next| !next.as_encoded_bytes().starts_with(b"--")) {
                return Some(PathBuf::from(folder));
            }
            continue;
        }
        if let Some(folder) = argument.to_str().and_then(|text| text.strip_prefix("--open=")) {
            return Some(PathBuf::from(folder));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::PathBuf;

    use super::{FALLBACK_STORAGE_ID, results_folder, storage_id};

    fn arguments(raw: &[&str]) -> Vec<OsString> {
        raw.iter().map(OsString::from).collect()
    }

    #[test]
    fn open_names_the_folder_after_it_in_either_form() {
        assert_eq!(
            results_folder(arguments(&["--open", "runs/sir"])),
            Some(PathBuf::from("runs/sir"))
        );
        assert_eq!(
            results_folder(arguments(&["--open=runs/sir"])),
            Some(PathBuf::from("runs/sir"))
        );
        assert_eq!(
            results_folder(arguments(&["--open"])),
            None,
            "a flag without its folder"
        );
        assert_eq!(results_folder(arguments(&[])), None);
    }

    /// The regression. A flag after `--open` used to be taken as the folder.
    #[test]
    fn open_never_takes_a_flag_for_its_folder() {
        assert_eq!(
            results_folder(arguments(&["--open", "--verbose"])),
            None,
            "a flag where the folder belongs"
        );
        assert_eq!(
            results_folder(arguments(&["--open", "--open=runs/sir"])),
            Some(PathBuf::from("runs/sir")),
            "the flag after a bare --open is read as a flag"
        );
    }

    /// The regression. An empty name stored the state beside every other app's, and a slash nested folders.
    #[test]
    fn a_product_name_becomes_a_folder_name() {
        assert_eq!(storage_id("Henad"), None, "the official app keeps its folder");
        assert_eq!(storage_id("My Model"), None);
        assert_eq!(storage_id("Lab: models/v2").as_deref(), Some("Lab- models-v2"));
        assert_eq!(storage_id(" Model. ").as_deref(), Some("Model"));
        assert_eq!(storage_id("").as_deref(), Some(FALLBACK_STORAGE_ID));
        assert_eq!(storage_id("..").as_deref(), Some(FALLBACK_STORAGE_ID));
        assert_eq!(storage_id("/").as_deref(), Some("-"));
    }
}
