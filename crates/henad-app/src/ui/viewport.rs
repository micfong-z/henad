use crate::{HenadApp, icons::material_design_icons::MDI_CUBE_OFF_OUTLINE};
use egui::{ColorImage, RichText, TextureOptions};

pub fn viewport_panel(ctx: &egui::Context, app: &mut HenadApp) {
    egui::CentralPanel::default().show(ctx, |ui| {
        let Some(runner) = &app.runner else {
            ui.centered_and_justified(|ui| {
                ui.heading("Load a model to start simulation");
            });
            return;
        };

        let current_tick = runner.state().tick();
        let needs_update =
            app.rendering_enabled && app.last_rendered_tick != Some(current_tick);

        if needs_update {
            let Some(view) = runner.state().grid_view() else {
                ui.label("No grid view available");
                return;
            };

            let w = view.width as usize;
            let h = view.height as usize;
            let total = w * h;

            // Resize pixel buffer if needed
            let needed = total * 4;
            if app.pixel_buf.len() != needed {
                app.pixel_buf.resize(needed, 255);
            }

            // Convert cell states to RGBA via palette lookup
            #[cfg(not(target_arch = "wasm32"))]
            {
                use rayon::prelude::*;
                app.pixel_buf
                    .par_chunks_mut(4)
                    .zip(view.cells.par_iter())
                    .for_each(|(px, &cell)| {
                        let rgba = view.palette[cell as usize];
                        px.copy_from_slice(&rgba);
                    });
            }
            #[cfg(target_arch = "wasm32")]
            for (chunk, &cell) in app.pixel_buf.chunks_exact_mut(4).zip(view.cells.iter()) {
                chunk.copy_from_slice(&view.palette[cell as usize]);
            }

            let image = ColorImage::from_rgba_unmultiplied([w, h], &app.pixel_buf);

            // Create or update texture
            match &mut app.grid_texture {
                Some(tex) => {
                    tex.set(image, TextureOptions::NEAREST);
                }
                None => {
                    app.grid_texture =
                        Some(ctx.load_texture("grid", image, TextureOptions::NEAREST));
                }
            }

            app.last_rendered_tick = Some(current_tick);
        }

        if !app.rendering_enabled {
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(MDI_CUBE_OFF_OUTLINE).size(64.0));
                ui.heading("Rendering disabled");
            });
            return;
        }

        if let Some(tex) = &app.grid_texture {
            let size = tex.size_vec2();
            let available = ui.available_size();
            let tex_aspect = size.x / size.y;
            let panel_aspect = available.x / available.y;

            let display_size = if tex_aspect > panel_aspect {
                egui::Vec2::new(available.x, available.x / tex_aspect)
            } else {
                egui::Vec2::new(available.y * tex_aspect, available.y)
            };

            ui.centered_and_justified(|ui| {
                ui.image(egui::load::SizedTexture::new(tex.id(), display_size));
            });
        }
    });
}
