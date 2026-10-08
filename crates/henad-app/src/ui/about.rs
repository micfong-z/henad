//! The About window, and the build information it reports.

use egui::{ColorImage, Context, Id, Label, Modal, ScrollArea, TextureHandle, TextureOptions, Vec2};
use henad_compute::entry::ModelSet;
use henad_core::provenance::BuildInfo;

use crate::icons::material_design_icons::MDI_CONTENT_COPY;
use crate::options::Product;
use crate::state::AppState;
use crate::ui::kv_grid;

/// Tagline of the official app.
const TAGLINE: &str = "A massively parallel agent-based modelling engine.";

/// Henad's logo, drawn instead of the icon in the official app's About window.
const LOGO_PNG: &[u8] = include_bytes!("../../assets/henad-logo-transparent-256.png");
const LOGO_SIZE: f32 = 64.0;

const MODAL_WIDTH: f32 = 420.0;
const MODAL_MARGIN: f32 = 16.0;
const FOOTER_HEIGHT: f32 = 100.0;

/// One row of the About window, a label and its value.
type InfoRow = (&'static str, String);

pub fn about_modal(ctx: &Context, app: &mut AppState) {
    if !app.about_open {
        return;
    }

    let logo = logo_texture(ctx, app);
    let info = build_info(&app.product, &app.models);
    let product = &app.product;

    let screen = ctx.content_rect();
    let width = MODAL_WIDTH.min(screen.width() - 2.0 * MODAL_MARGIN);
    let logo_width = if logo.is_some() { LOGO_SIZE + MODAL_MARGIN } else { 0.0 };

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
                        if let Some(logo) = &logo {
                            ui.add(egui::Image::new(logo).fit_to_exact_size(Vec2::splat(LOGO_SIZE)));
                            ui.add_space(8.0);
                        }
                        ui.vertical(|ui| {
                            ui.set_max_width((width - logo_width).max(0.0));
                            ui.heading(&product.name);
                            if product.official {
                                ui.add(Label::new(TAGLINE).wrap());
                            }
                            ui.horizontal(|ui| {
                                if let Some(url) = &product.source_url {
                                    ui.hyperlink_to("Source code", url);
                                }
                                if let Some(url) = &product.documentation_url {
                                    ui.hyperlink_to("Documentation", url);
                                }
                            });
                        });
                    });

                    ui.add_space(12.0);
                    kv_grid(ui, "about_grid").show(ui, |ui, rows| {
                        let mut previous = "";
                        for (label, value) in &info {
                            // A label repeated over several rows, such as Models, is drawn once.
                            ui.label(if *label == previous { "" } else { *label });
                            ui.add(Label::new(value).selectable(true));
                            rows.end_row(ui);
                            previous = label;
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
                    ui.ctx().copy_text(clipboard_text(&product.name, &info));
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

/// Returns the About window's image, decoded on the first call only. `None` for an icon that is not a PNG.
fn logo_texture(ctx: &Context, app: &AppState) -> Option<TextureHandle> {
    let png = if app.product.official {
        LOGO_PNG
    } else {
        app.product.icon_png
    };
    app.logo_texture
        .get_or_init(|| decode_png(png).map(|image| ctx.load_texture("henad_logo", image, TextureOptions::LINEAR)))
        .clone()
}

fn decode_png(png: &[u8]) -> Option<ColorImage> {
    let rgba = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    Some(ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}

/// Returns the rows of the About window: the product's build, the Henad it is built on unless it is Henad's own app,
/// and one row per crate that registered models.
fn build_info(product: &Product, models: &ModelSet) -> Vec<InfoRow> {
    let host = &product.host;
    let mut rows = vec![
        ("Version", host.version().to_owned()),
        ("Commit", commit_text(host.commit(), host.commit_date(), host.dirty())),
        (
            "Sources",
            host.source_hash()
                .map_or_else(|| "Unknown".to_owned(), |hash| format!("{hash:016x}")),
        ),
        (
            "Build",
            if cfg!(debug_assertions) { "Debug" } else { "Release" }.to_owned(),
        ),
    ];
    if let Some(license) = &product.license {
        rows.push(("License", license.clone()));
    }
    if !product.official {
        rows.push((
            "Built on",
            engine_text(&henad_explore::ENGINE_BUILD, henad_core::__VERSION),
        ));
    }
    let mut sources: Vec<String> = Vec::new();
    for entry in models {
        let source = entry.source();
        let text = format!(
            "{} {} ({})",
            source.package(),
            source.version(),
            build_text(source.commit(), source.dirty(), source.source_hash())
        );
        if !sources.contains(&text) {
            sources.push(text);
        }
    }
    rows.extend(sources.into_iter().map(|text| ("Models", text)));
    rows
}

/// Returns a commit as the About window shows it, as in "5feb3e9a (2026-10-02), modified".
fn commit_text(commit: &str, date: &str, dirty: Option<bool>) -> String {
    let mut text = match (commit, date) {
        ("", _) => "Unknown commit".to_owned(),
        (commit, "") => commit.to_owned(),
        (commit, date) => format!("{commit} ({date})"),
    };
    if dirty == Some(true) {
        text.push_str(", modified");
    }
    text
}

/// Returns a crate's build as the Models and Built on rows show it: its commit, or the hash of its sources when its
/// build script stamps no commit, as in "5feb3e92, modified" or "sources 0cf285d3aa391254".
fn build_text(commit: &str, dirty: Option<bool>, source_hash: Option<u64>) -> String {
    let mut text = match (commit, source_hash) {
        ("", Some(hash)) => format!("sources {hash:016x}"),
        ("", None) => "unknown build".to_owned(),
        (commit, _) => commit.to_owned(),
    };
    if dirty == Some(true) {
        text.push_str(", modified");
    }
    text
}

/// Returns the Henad build that a product is built on, as in "Henad 0.3.0 (773a7a5b)", with `core_version`, the
/// henad-core version, beside it when it differs.
fn engine_text(engine: &BuildInfo, core_version: &str) -> String {
    let mut text = format!(
        "Henad {} ({})",
        engine.version(),
        build_text(engine.commit(), engine.dirty(), engine.source_hash())
    );
    if core_version != engine.version() {
        text.push_str(&format!(", henad-core {core_version}"));
    }
    text
}

/// Returns the product's name and one line per row, so a pasted report reads as a list.
fn clipboard_text(product: &str, info: &[InfoRow]) -> String {
    let rows: String = info
        .iter()
        .map(|(label, value)| format!("{label}: {value}\n"))
        .collect();
    format!("{product}\n{rows}")
}

#[cfg(test)]
mod tests {
    use henad_compute::entry::ModelSet;

    use super::{LOGO_PNG, build_info, clipboard_text, decode_png};
    use crate::options::AppOptions;

    #[test]
    fn the_bundled_logo_decodes_to_a_square() {
        let logo = decode_png(LOGO_PNG).expect("the bundled logo is a PNG");
        let [width, height] = logo.size;
        assert_eq!(width, height, "the logo is a square, got {width}x{height}");
    }

    /// One line per row after the product's name, so a pasted report reads as a list.
    #[test]
    fn copy_covers_every_row() {
        let models = henad_models::example_models();
        let options = AppOptions::new(models.clone(), "Henad", henad_core::build_info!()).license("MIT");
        let info = build_info(&options.product, &models);
        let text = clipboard_text("Henad", &info);
        assert_eq!(text.lines().count(), info.len() + 1);
        for (label, _) in &info {
            assert!(text.contains(label), "{label} missing from the copied text");
        }
    }

    /// The official app is Henad, and a product built on it shows the engine. Each crate that registered models is
    /// listed once.
    #[test]
    fn a_product_other_than_henad_names_the_engine_it_is_built_on() {
        let models = henad_models::example_models();
        let labels = |info: &[(&'static str, String)]| info.iter().map(|(label, _)| *label).collect::<Vec<_>>();

        let official = AppOptions::new(models.clone(), "Henad", henad_core::build_info!()).__official();
        let info = build_info(&official.product, &models);
        assert_eq!(
            labels(&info),
            ["Version", "Commit", "Sources", "Build", "Models"],
            "the official app, ten models from one crate"
        );
        assert!(info[4].1.starts_with("henad-models "), "{}", info[4].1);
        assert!(
            !info[4].1.contains("unknown build"),
            "henad-models stamps its sources: {}",
            info[4].1
        );

        // Only the official flag marks Henad's app. This host build belongs to henad-app.
        let other = AppOptions::new(
            ModelSet::new(henad_core::build_info!()),
            "Vote",
            henad_core::build_info!(),
        );
        let info = build_info(&other.product, &ModelSet::new(henad_core::build_info!()));
        assert_eq!(labels(&info), ["Version", "Commit", "Sources", "Build", "Built on"]);
        assert!(info[4].1.starts_with("Henad "), "{}", info[4].1);
    }
}
