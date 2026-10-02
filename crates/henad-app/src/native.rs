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
/// Panics off the main thread on macOS, where winit refuses to start.
pub fn run_native(options: AppOptions) -> Result<(), AppError> {
    options.check_opening().map_err(AppError::opening)?;

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([400.0, 300.0])
        .with_min_inner_size([300.0, 220.0]);
    match eframe::icon_data::from_png_bytes(options.product.icon_png) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(error) => log::warn!("the app icon is no PNG and is left out: {error}"),
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

    use super::results_folder;

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
}
