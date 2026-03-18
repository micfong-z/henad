use crate::{HenadApp, icons::material_design_icons::MDI_CUBE_OFF_OUTLINE};
use egui::{ColorImage, RichText, TextureOptions};
use henad_core::topology::TopologyHint;

pub fn viewport_panel(ctx: &egui::Context, app: &mut HenadApp) {
    egui::CentralPanel::default().show(ctx, |ui| {
        let Some(runner) = &app.runner else {
            ui.centered_and_justified(|ui| {
                ui.heading("Load a model to start simulation");
            });
            return;
        };

        let current_tick = runner.state().tick();
        let needs_update = app.rendering_enabled && app.last_rendered_tick != Some(current_tick);

        if needs_update {
            if let Some(view) = runner.state().grid_view() {
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
            } else if app
                .selected_topology_hint()
                .is_some_and(|h| h == TopologyHint::PointCloud)
            {
                let copied = runner.state().point_view().map(|view| {
                    (
                        view.pos_x.to_vec(),
                        view.pos_y.to_vec(),
                        view.world_w,
                        view.world_h,
                    )
                });
                if let Some((pos_x, pos_y, world_w, world_h)) = copied {
                    render_density_heatmap(ctx, app, &pos_x, &pos_y, world_w, world_h);
                }
                app.last_rendered_tick = Some(current_tick);
            } else {
                ui.label("No grid view available");
                return;
            }
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

const DENSITY_W: usize = 512;
const DENSITY_H: usize = 512;

/// 5-stop piecewise linear approximation of the Inferno colormap.
#[inline]
fn inferno(t: f32) -> [u8; 4] {
    const STOPS: [[u8; 3]; 5] = [
        [0, 0, 4],
        [64, 4, 104],
        [183, 55, 121],
        [251, 136, 97],
        [252, 255, 164],
    ];

    let t = t.clamp(0.0, 1.0);
    let scaled = t * 4.0;
    let seg = (scaled as usize).min(3);
    let frac = scaled - seg as f32;

    let a = STOPS[seg];
    let b = STOPS[seg + 1];

    let r = a[0] as f32 + (b[0] as f32 - a[0] as f32) * frac;
    let g = a[1] as f32 + (b[1] as f32 - a[1] as f32) * frac;
    let bl = a[2] as f32 + (b[2] as f32 - a[2] as f32) * frac;

    [r as u8, g as u8, bl as u8, 255]
}

fn render_density_heatmap(
    ctx: &egui::Context,
    app: &mut HenadApp,
    pos_x: &[f32],
    pos_y: &[f32],
    world_w: f32,
    world_h: f32,
) {
    let num_pixels = DENSITY_W * DENSITY_H;
    let inv_world_w = DENSITY_W as f32 / world_w;
    let inv_world_h = DENSITY_H as f32 / world_h;

    #[cfg(not(target_arch = "wasm32"))]
    let density = {
        use rayon::prelude::*;

        let n_threads = rayon::current_num_threads().max(1);
        let chunk_size = pos_x.len().div_ceil(n_threads);

        let partial: Vec<Vec<u32>> = pos_x
            .par_chunks(chunk_size)
            .zip(pos_y.par_chunks(chunk_size))
            .map(|(xs, ys)| {
                let mut buf = vec![0u32; num_pixels];
                for (&x, &y) in xs.iter().zip(ys.iter()) {
                    let px = ((x * inv_world_w) as usize).min(DENSITY_W - 1);
                    let py = ((y * inv_world_h) as usize).min(DENSITY_H - 1);
                    buf[py * DENSITY_W + px] += 1;
                }
                buf
            })
            .collect();

        let mut density = vec![0u32; num_pixels];
        for buf in &partial {
            for (d, &p) in density.iter_mut().zip(buf.iter()) {
                *d += p;
            }
        }
        density
    };

    #[cfg(target_arch = "wasm32")]
    let density = {
        let mut density = vec![0u32; num_pixels];
        for (&x, &y) in pos_x.iter().zip(pos_y.iter()) {
            let px = ((x * inv_world_w) as usize).min(DENSITY_W - 1);
            let py = ((y * inv_world_h) as usize).min(DENSITY_H - 1);
            density[py * DENSITY_W + px] += 1;
        }
        density
    };

    // let frame_max = density.iter().copied().max().unwrap_or(1).max(1) as f32;
    // if frame_max > app.density_max {
    //     app.density_max = frame_max;
    // }

    let max_density = app.density_max;

    app.pixel_buf.resize(num_pixels * 4, 255);
    for (chunk, &d) in app.pixel_buf.chunks_exact_mut(4).zip(density.iter()) {
        let color = inferno(d as f32 / max_density);
        chunk[0] = color[0];
        chunk[1] = color[1];
        chunk[2] = color[2];
        chunk[3] = color[3];
    }

    let image = egui::ColorImage::from_rgba_unmultiplied([DENSITY_W, DENSITY_H], &app.pixel_buf);

    match &mut app.grid_texture {
        Some(tex) => {
            tex.set(image, TextureOptions::LINEAR);
        }
        None => {
            app.grid_texture =
                Some(ctx.load_texture("density_heatmap", image, TextureOptions::LINEAR));
        }
    }
}
