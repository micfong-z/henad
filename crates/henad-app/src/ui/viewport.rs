use std::sync::Arc;

use crate::{icons::material_design_icons::MDI_CUBE_OFF_OUTLINE, state::AppState};
use eframe::egui_wgpu;
use egui::{ColorImage, RichText, TextureOptions};
use egui_wgpu::{CallbackResources, CallbackTrait};
use henad_compute::gpu::GpuDisplay;
use henad_compute::snapshot::SnapshotView;

/// Paints a GPU model's display texture straight into the viewport.
///
/// A GPU model's cells never reach the CPU, so there is no `ColorImage` to upload — the sim
/// thread has already rendered the grid into a texture, and all that is left is to sample it.
/// The callback carries its own `Arc<GpuDisplay>` rather than looking resources up in egui's
/// type-keyed `CallbackResources`, which is what makes model teardown safe: if the user switches
/// models while this frame is still in flight, this `Arc` keeps the pipeline and texture alive
/// until the render pass is done with them.
struct GpuViewportPaint {
    display: Arc<GpuDisplay>,
}

impl CallbackTrait for GpuViewportPaint {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &CallbackResources,
    ) {
        render_pass.set_pipeline(&self.display.render_pipeline);
        render_pass.set_bind_group(0, &self.display.render_bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

/// Allocates an aspect-fitted rect and hands it to the GPU paint callback.
fn paint_gpu_view(ui: &mut egui::Ui, display: Arc<GpuDisplay>) {
    let size = fit_aspect(ui.available_size(), display.width as f32, display.height as f32);
    ui.centered_and_justified(|ui| {
        let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            rect,
            GpuViewportPaint { display },
        ));
    });
}

/// Fits `size` inside `available` while preserving aspect ratio.
fn fit_aspect(available: egui::Vec2, width: f32, height: f32) -> egui::Vec2 {
    let tex_aspect = width / height;
    let panel_aspect = available.x / available.y;
    if tex_aspect > panel_aspect {
        egui::Vec2::new(available.x, available.x / tex_aspect)
    } else {
        egui::Vec2::new(available.y * tex_aspect, available.y)
    }
}

/// Draws the view and records its cost
pub fn viewport_ui(ui: &mut egui::Ui, app: &mut AppState) {
    #[cfg(not(target_arch = "wasm32"))]
    let render_start = std::time::Instant::now();

    ui.checkbox(&mut app.rendering_enabled, "Rendering");

    draw_view(ui, app);

    #[cfg(not(target_arch = "wasm32"))]
    {
        app.timings.frame_render_ms = render_start.elapsed().as_secs_f64() * 1000.0;
    }
}

fn draw_view(ui: &mut egui::Ui, app: &mut AppState) {
    let ctx = ui.ctx().clone();

    if app.snapshot.is_none() {
        ui.centered_and_justified(|ui| {
            ui.heading("Load a model to start simulation");
        });
        return;
    }

    if !app.rendering_enabled {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(MDI_CUBE_OFF_OUTLINE).size(64.0));
            ui.heading("Rendering disabled");
        });
        return;
    }

    // --- GPU path: nothing to convert or upload, just sample the texture the sim thread
    // already wrote. Branches on the snapshot *variant*, not on the topology hint — a GPU
    // Game of Life is still `TopologyHint::Grid2D`, it just gets its pixels differently.
    if let Some(SnapshotView::Gpu(gpu)) = app.snapshot.as_ref().map(|s| &s.view) {
        paint_gpu_view(ui, Arc::clone(&gpu.display));
        return;
    }

    let current_tick = app.snapshot.as_ref().map_or(0, |s| s.tick);
    let needs_update = app.last_rendered_tick != Some(current_tick);

    if needs_update {
        // Take snapshot temporarily to avoid borrow conflicts with app
        let snap = app.snapshot.take();
        if let Some(snapshot) = snap {
            match &snapshot.view {
                SnapshotView::Grid(grid) => {
                    let w = grid.width as usize;
                    let h = grid.height as usize;
                    let total = w * h;

                    let needed = total * 4;
                    if app.pixel_buf.len() != needed {
                        app.pixel_buf.resize(needed, 255);
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        use rayon::prelude::*;
                        app.pixel_buf
                            .par_chunks_mut(4)
                            .zip(grid.cells.par_iter())
                            .for_each(|(px, &cell)| {
                                let rgba = grid.palette[cell as usize];
                                px.copy_from_slice(&rgba);
                            });
                    }
                    #[cfg(target_arch = "wasm32")]
                    for (chunk, &cell) in app.pixel_buf.chunks_exact_mut(4).zip(grid.cells.iter()) {
                        chunk.copy_from_slice(&grid.palette[cell as usize]);
                    }

                    let image = ColorImage::from_rgba_unmultiplied([w, h], &app.pixel_buf);

                    match &mut app.grid_texture {
                        Some(tex) => {
                            tex.set(image, TextureOptions::NEAREST);
                        }
                        None => {
                            app.grid_texture = Some(ctx.load_texture("grid", image, TextureOptions::NEAREST));
                        }
                    }
                }
                SnapshotView::Points(points) => {
                    render_density_heatmap(&ctx, app, &points.pos_x, &points.pos_y, points.world_w, points.world_h);
                }
                // Handled above, before any CPU-side pixel work.
                SnapshotView::Gpu(_) => {}
                SnapshotView::None => {
                    app.snapshot = Some(snapshot);
                    ui.label("No view available");
                    return;
                }
            }
            app.last_rendered_tick = Some(current_tick);
            app.snapshot = Some(snapshot);
        }
    }

    if let Some(tex) = &app.grid_texture {
        let size = tex.size_vec2();
        let display_size = fit_aspect(ui.available_size(), size.x, size.y);

        ui.centered_and_justified(|ui| {
            ui.image(egui::load::SizedTexture::new(tex.id(), display_size));
        });
    }
}

const DENSITY_W: usize = 512;
const DENSITY_H: usize = 512;

/// 5-stop piecewise linear approximation of the Inferno colormap.
#[inline]
fn inferno(t: f32) -> [u8; 4] {
    const STOPS: [[u8; 3]; 5] = [[0, 0, 4], [64, 4, 104], [183, 55, 121], [251, 136, 97], [252, 255, 164]];

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
    app: &mut AppState,
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
            app.grid_texture = Some(ctx.load_texture("density_heatmap", image, TextureOptions::LINEAR));
        }
    }
}
