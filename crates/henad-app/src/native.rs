//! The app's window on a desktop, and the results folder that a command line specifies.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::HenadApp;
use crate::init::wgpu_configuration;
use crate::options::{AppError, AppOptions};
use crate::state::AppState;

/// Opens the app window and returns when it closes.
///
/// # Errors
///
/// Returns an error before the window opens when the options' models cannot serve their opening, and when eframe
/// cannot open the window or fails while it is open.
///
/// # Panics
///
/// Panics when called off the main thread. winit allows an event loop only on the main thread.
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
        Box::new(|cc| {
            // winit has installed its menu by the time eframe creates the app.
            #[cfg(target_os = "macos")]
            remove_menu_quit();
            Ok(Box::new(HenadApp::new(cc, options)))
        }),
    )
    .map_err(AppError::eframe)
}

/// Holds a close request of the window while [`AppState::hold_close`] holds.
///
/// A held request leaves the window open, and the unsaved changes dialog asks about the edits. egui requests the close
/// on Cmd+Q, Ctrl+Q off macOS, through [`egui::Options::quit_shortcuts`].
pub(crate) fn guard_close(ctx: &egui::Context, state: &mut AppState) {
    if ctx.input(|input| input.viewport().close_requested()) && state.hold_close() {
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
    }
}

/// Removes the Quit item from the app menu that winit installs on macOS, with the separator before it.
///
/// The item terminates the app at once, past the unsaved changes dialog. Without the item, Cmd+Q reaches egui as a key
/// press, and egui requests the close that [`guard_close`] holds. Logs a warning when the menu holds no Quit item.
#[cfg(target_os = "macos")]
fn remove_menu_quit() {
    use objc2::{MainThreadMarker, sel};
    use objc2_app_kit::NSApplication;

    // `run_native` runs on the main thread, and a menu bar is missing only where winit installed none.
    let Some(menu_bar) =
        MainThreadMarker::new().and_then(|main_thread| NSApplication::sharedApplication(main_thread).mainMenu())
    else {
        return;
    };
    for menu in (0..menu_bar.numberOfItems()).filter_map(|index| menu_bar.itemAtIndex(index)?.submenu()) {
        let quit = (0..menu.numberOfItems()).find(|&index| {
            menu.itemAtIndex(index)
                .is_some_and(|item| item.action() == Some(sel!(terminate:)))
        });
        let Some(quit) = quit else {
            continue;
        };
        menu.removeItemAtIndex(quit);
        // Otherwise the separator before the item ends the menu with a line.
        if quit > 0
            && quit == menu.numberOfItems()
            && menu.itemAtIndex(quit - 1).is_some_and(|item| item.isSeparatorItem())
        {
            menu.removeItemAtIndex(quit - 1);
        }
        return;
    }
    log::warn!("the app menu holds no Quit item to remove");
}

/// Folder name for eframe's stored state when the product name is empty after [`storage_id`] cleans it.
const FALLBACK_STORAGE_ID: &str = "henad-app";

/// Returns the folder name that eframe stores the state of product `name` under, or `None` when `name` is usable
/// as it is.
///
/// Each character that a folder name cannot contain on some platform becomes `-`, and leading and trailing spaces and
/// dots are removed. A name that ends up empty uses [`FALLBACK_STORAGE_ID`].
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

/// Returns the folder that `--open DIR` or `--open=DIR` specifies among `arguments`, the command line without the
/// program.
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

    /// `--open --verbose` opens no folder named `--verbose`.
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

    /// An empty name would store the state beside every other app's state, and a slash would nest folders.
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
