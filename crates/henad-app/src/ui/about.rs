//! The About window, and the build information it reports.

use egui::{ColorImage, Context, Id, Label, Modal, ScrollArea, TextureHandle, TextureOptions, Vec2};

use crate::icons::material_design_icons::MDI_CONTENT_COPY;
use crate::state::AppState;
use crate::ui::kv_grid;

pub const SOURCE_URL: &str = "https://github.com/micfong-z/henad";
pub const DOCS_URL: &str = "https://micfong-z.github.io/henad/";

const TAGLINE: &str = "A massively parallel agent-based modelling engine.";

const LOGO_PNG: &[u8] = include_bytes!("../../../../assets/henad-logo-transparent-256.png");
const LOGO_SIZE: f32 = 64.0;

const MODAL_WIDTH: f32 = 420.0;
const MODAL_MARGIN: f32 = 16.0;
const FOOTER_HEIGHT: f32 = 100.0;

pub fn about_modal(ctx: &Context, app: &mut AppState) {
    if !app.about_open {
        return;
    }

    let logo = logo_texture(ctx, app);
    let info = build_info();

    let screen = ctx.content_rect();
    let width = MODAL_WIDTH.min(screen.width() - 2.0 * MODAL_MARGIN);

    let mut dismissed = false;
    let id = Id::new("henad_about_modal");
    let response = Modal::new(id)
        .area(Modal::default_area(id).constrain(true))
        .show(ctx, |ui| {
            ui.set_width(width);

            ScrollArea::vertical()
                .max_height((screen.height() - FOOTER_HEIGHT).max(LOGO_SIZE))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Image::new(&logo).fit_to_exact_size(Vec2::splat(LOGO_SIZE)));
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.set_max_width((width - LOGO_SIZE - MODAL_MARGIN).max(0.0));
                            ui.heading("Henad");
                            ui.add(Label::new(TAGLINE).wrap());
                            ui.horizontal(|ui| {
                                ui.hyperlink_to("Source code", SOURCE_URL);
                                ui.hyperlink_to("Documentation", DOCS_URL);
                            });
                        });
                    });

                    ui.add_space(12.0);
                    kv_grid(ui, "about_grid").show(ui, |ui| {
                        for (label, value) in &info {
                            ui.label(*label);
                            ui.add(Label::new(value).selectable(true));
                            ui.end_row();
                        }
                    });
                });

            ui.add_space(8.0);
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{MDI_CONTENT_COPY} Copy"))
                    .on_hover_text("Copy build info to clipboard")
                    .clicked()
                {
                    ui.ctx().copy_text(clipboard_text(&info));
                }
                if ui.button("Close").clicked() {
                    dismissed = true;
                }
            });
        });

    if dismissed || response.should_close() {
        app.about_open = false;
    }
}

fn logo_texture(ctx: &Context, app: &mut AppState) -> TextureHandle {
    app.logo_texture
        .get_or_insert_with(|| ctx.load_texture("henad_logo", decode_logo(), TextureOptions::LINEAR))
        .clone()
}

fn decode_logo() -> ColorImage {
    let rgba = image::load_from_memory_with_format(LOGO_PNG, image::ImageFormat::Png)
        .expect("the bundled logo is a valid PNG")
        .into_rgba8();
    ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())
}

fn build_info() -> Vec<(&'static str, String)> {
    let commit = match (env!("HENAD_COMMIT"), env!("HENAD_COMMIT_DATE")) {
        ("", _) => "Unknown".to_owned(),
        (commit, "") => commit.to_owned(),
        (commit, date) => format!("{commit} ({date})"),
    };

    vec![
        ("Version", env!("CARGO_PKG_VERSION").to_owned()),
        ("Commit", commit),
        (
            "Build",
            if cfg!(debug_assertions) { "Debug" } else { "Release" }.to_owned(),
        ),
        ("License", env!("CARGO_PKG_LICENSE").to_owned()),
    ]
}

fn clipboard_text(info: &[(&'static str, String)]) -> String {
    info.iter()
        .map(|(label, value)| format!("{label}: {value}\n"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{build_info, clipboard_text, decode_logo};

    #[test]
    fn the_bundled_logo_decodes_to_a_square() {
        let logo = decode_logo();
        let [width, height] = logo.size;
        assert_eq!(width, height, "the logo is a square, got {width}x{height}");
    }

    /// One line per row, so a pasted report reads as a list.
    #[test]
    fn copy_covers_every_row() {
        let info = build_info();
        let text = clipboard_text(&info);
        assert_eq!(text.lines().count(), info.len());
        for (label, _) in &info {
            assert!(text.contains(label), "{label} missing from the copied text");
        }
    }
}
