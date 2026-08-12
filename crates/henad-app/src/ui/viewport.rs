use std::sync::Arc;

use crate::state::PointRenderMode;
use crate::ui::agent_layer::AgentLayer;
use crate::{icons::material_design_icons::MDI_CUBE_OFF_OUTLINE, state::AppState};
use eframe::egui_wgpu;
use egui::{ColorImage, RichText, TextureOptions};
use egui_wgpu::{CallbackResources, CallbackTrait};
use henad_compute::gpu::GpuDisplay;
use henad_compute::snapshot::{CpuLayers, GpuSnapshot, GridSnapshot, PointSnapshot, SnapshotView};

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

/// Composites whichever GPU layers the model published into one rect, field first and agents over
/// the top, same order as the CPU path.
///
/// Takes `app` because the agent pipeline is built lazily on first use.
fn paint_gpu_view(ui: &mut egui::Ui, app: &mut AppState, gpu: &GpuSnapshot) {
    // The field fixes the pixel shape when there is one, as `layer_extent` does for CPU models.
    let extent = gpu.display.as_ref().map_or_else(
        || gpu.agents.as_ref().map(|a| (a.world_w, a.world_h)),
        |d| Some((d.width as f32, d.height as f32)),
    );
    let Some((world_w, world_h)) = extent else {
        return;
    };
    if gpu.agents.is_some() {
        ensure_agent_layer(app);
    }

    let size = fit_aspect(ui.available_size(), world_w, world_h);
    ui.centered_and_justified(|ui| {
        let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());

        if let Some(display) = &gpu.display {
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                GpuViewportPaint {
                    display: Arc::clone(display),
                },
            ));
        }
        if let (Some(agents), Some(layer)) = (&gpu.agents, &app.agent_layer) {
            layer.paint_gpu(ui, rect, agents);
        }
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

    let mode_before = app.point_render_mode;
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.rendering_enabled, "Rendering");

        if app.rendering_enabled && has_points(app) {
            ui.separator();
            ui.label("Agents:");
            ui.selectable_value(&mut app.point_render_mode, PointRenderMode::Agents, "Sprites");
            ui.selectable_value(&mut app.point_render_mode, PointRenderMode::Density, "Density");
        }
    });
    if app.point_render_mode != mode_before {
        // The tick has not moved, but what we draw from it has.
        app.last_rendered_tick = None;
    }

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

    // GPU path, nothing to convert or upload. Branches on the snapshot variant rather than the
    // topology hint, since a GPU Game of Life is still `TopologyHint::GRID`.
    //
    // Taken out for the duration because `paint_gpu_view` needs `app` for the agent pipeline.
    if matches!(app.snapshot.as_ref().map(|s| &s.view), Some(SnapshotView::Gpu(_))) {
        let Some(snapshot) = app.snapshot.take() else {
            return;
        };
        if let SnapshotView::Gpu(gpu) = &snapshot.view {
            paint_gpu_view(ui, app, gpu);
        }
        app.snapshot = Some(snapshot);
        return;
    }

    let current_tick = app.snapshot.as_ref().map_or(0, |s| s.tick);
    let needs_update = app.last_rendered_tick != Some(current_tick);

    // Taken out for the duration to avoid borrow conflicts with the rest of `app`.
    let Some(snapshot) = app.snapshot.take() else {
        return;
    };
    // The GPU arm returned above, before any CPU-side pixel work.
    let SnapshotView::Cpu(layers) = &snapshot.view else {
        app.snapshot = Some(snapshot);
        return;
    };

    if layers.is_empty() {
        app.snapshot = Some(snapshot);
        ui.label("No view available");
        return;
    }

    if needs_update {
        if let Some(grid) = &layers.grid {
            upload_grid(&ctx, app, grid);
        }
        if let Some(points) = &layers.points {
            match app.point_render_mode {
                PointRenderMode::Agents => upload_agents(app, points),
                PointRenderMode::Density => {
                    render_density_heatmap(&ctx, app, &points.pos_x, &points.pos_y, points.world_w, points.world_h);
                }
            }
        }
        app.last_rendered_tick = Some(current_tick);
    }

    // Both layers share one rect so they stay registered. See `PointView`'s docs.
    let Some((world_w, world_h)) = layer_extent(layers) else {
        app.snapshot = Some(snapshot);
        return;
    };
    let display_size = fit_aspect(ui.available_size(), world_w, world_h);

    ui.centered_and_justified(|ui| {
        let (rect, _response) = ui.allocate_exact_size(display_size, egui::Sense::hover());

        // Painter order is the compositing order, field first and agents over the top.
        if let Some(tex) = &app.grid_texture {
            ui.painter().image(tex.id(), rect, UV_FULL, egui::Color32::WHITE);
        }
        if layers.points.is_some() {
            match app.point_render_mode {
                PointRenderMode::Agents => {
                    if let Some(layer) = &app.agent_layer {
                        layer.paint(ui, rect, world_w, world_h);
                    }
                }
                PointRenderMode::Density => {
                    if let Some(tex) = &app.density_texture {
                        ui.painter().image(tex.id(), rect, UV_FULL, egui::Color32::WHITE);
                    }
                }
            }
        }
    });

    app.snapshot = Some(snapshot);
}

const UV_FULL: egui::Rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

/// Extent the shared rect is fitted to, and the world size handed to the agent shader.
///
/// The field wins when there is one, since it is the layer with a fixed pixel shape.
fn layer_extent(layers: &CpuLayers) -> Option<(f32, f32)> {
    if let Some(grid) = &layers.grid {
        return Some((grid.width as f32, grid.height as f32));
    }
    let points = layers.points.as_ref()?;
    Some((points.world_w, points.world_h))
}

/// Whether the mode selector applies at all.
fn has_points(app: &AppState) -> bool {
    matches!(
        app.snapshot.as_ref().map(|s| &s.view),
        Some(SnapshotView::Cpu(layers)) if layers.points.is_some()
    )
}

fn upload_grid(ctx: &egui::Context, app: &mut AppState, grid: &GridSnapshot) {
    let size = [grid.width as usize, grid.height as usize];

    #[cfg(not(target_arch = "wasm32"))]
    let pixels: Vec<egui::Color32> = {
        use rayon::prelude::*;
        grid.cells
            .par_iter()
            .map(|&cell| palette_color(grid.palette, cell))
            .collect()
    };
    #[cfg(target_arch = "wasm32")]
    let pixels: Vec<egui::Color32> = grid
        .cells
        .iter()
        .map(|&cell| palette_color(grid.palette, cell))
        .collect();

    let image = ColorImage::new(size, pixels);

    match &mut app.grid_texture {
        Some(tex) => tex.set(image, TextureOptions::NEAREST),
        None => app.grid_texture = Some(ctx.load_texture("grid", image, TextureOptions::NEAREST)),
    }
}

/// Built on first use, and shared by both backends.
fn ensure_agent_layer(app: &mut AppState) {
    if app.agent_layer.is_none() {
        app.agent_layer = Some(AgentLayer::new(
            &app.render_ctx.device,
            &app.render_ctx.queue,
            app.render_ctx.target_format,
        ));
    }
}

fn upload_agents(app: &mut AppState, points: &PointSnapshot) {
    ensure_agent_layer(app);
    if let Some(layer) = &mut app.agent_layer {
        layer.upload(points);
    }
}

const DENSITY_W: usize = 512;
const DENSITY_H: usize = 512;

#[inline]
fn palette_color(palette: &[[u8; 4]], cell: u8) -> egui::Color32 {
    let [r, g, b, a] = palette[cell as usize];
    egui::Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// 5-stop piecewise linear approximation of the Inferno colormap.
#[inline]
fn inferno(t: f32) -> egui::Color32 {
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

    egui::Color32::from_rgb(r as u8, g as u8, bl as u8)
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
    // Transparent rather than black where empty, so a field underneath still shows through.
    let pixels: Vec<egui::Color32> = density
        .iter()
        .map(|&d| {
            if d == 0 {
                egui::Color32::TRANSPARENT
            } else {
                inferno(d as f32 / max_density)
            }
        })
        .collect();
    let image = ColorImage::new([DENSITY_W, DENSITY_H], pixels);

    match &mut app.density_texture {
        Some(tex) => {
            tex.set(image, TextureOptions::LINEAR);
        }
        None => {
            app.density_texture = Some(ctx.load_texture("density_heatmap", image, TextureOptions::LINEAR));
        }
    }
}
