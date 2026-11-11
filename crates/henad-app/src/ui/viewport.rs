use std::sync::Arc;

use crate::state::PointRenderMode;
use crate::ui::agent_layer::{AgentLayer, padded_palette};
use crate::ui::painted::{Painted, painted};
use crate::{icons::material_design_icons::MDI_CUBE_OFF_OUTLINE, state::AppState};
use eframe::egui_wgpu;
use egui::{ColorImage, RichText, TextureOptions};
use egui_wgpu::{CallbackResources, CallbackTrait};
use henad_compute::display_scale::{display_dims, source_row};
use henad_compute::gpu::GpuDisplay;
use henad_compute::snapshot::{
    CpuLayers, EdgeSnapshot, GpuSnapshot, GridSnapshot, PointSnapshot, Snapshot, SnapshotView,
};

/// Paints a GPU model's display texture straight into the viewport.
///
/// A GPU model's cells never reach the CPU, so there is no `ColorImage` to upload. The sim thread
/// has already rendered the grid into a texture, and all that is left is to sample it.
///
/// The callback carries its own `Arc<GpuDisplay>` instead of a lookup in egui's type-keyed `CallbackResources`. A
/// model switch mid-frame then leaves the pipeline and texture alive until the render pass is done with them.
struct GpuViewportPaint {
    display: Painted<Arc<GpuDisplay>>,
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
/// the top, in the order the CPU path draws them.
///
/// Takes `app` because the agent pipeline is built lazily on first use.
fn paint_gpu_view(ui: &mut egui::Ui, app: &mut AppState, gpu: &GpuSnapshot, extent: ViewExtent) {
    if gpu.agents.is_some() {
        ensure_agent_layer(app);
    }

    let size = fit_aspect(ui.available_size(), extent.width, extent.height);
    ui.centered_and_justified(|ui| {
        let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());

        if let Some(display) = &gpu.display {
            ui.painter().add(egui_wgpu::Callback::new_paint_callback(
                rect,
                GpuViewportPaint {
                    display: painted(Arc::clone(display)),
                },
            ));
        }
        if let (Some(agents), Some(layer)) = (&gpu.agents, &app.agent_layer) {
            layer.paint_gpu(ui, rect, agents);
        }
    });
}

/// Returns the largest size with the aspect ratio of `width` by `height` that fits inside `available`.
fn fit_aspect(available: egui::Vec2, width: f32, height: f32) -> egui::Vec2 {
    let view_aspect = width / height;
    let panel_aspect = available.x / available.y;
    if view_aspect > panel_aspect {
        egui::Vec2::new(available.x, available.x / view_aspect)
    } else {
        egui::Vec2::new(available.y * view_aspect, available.y)
    }
}

/// Draws the viewport and records its cost.
pub fn viewport_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let render_start = web_time::Instant::now();

    let mode_before = app.point_render_mode;
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.rendering_enabled, "Rendering");

        if app.rendering_enabled && has_points(app) {
            ui.separator();
            ui.label("Agents:");
            ui.selectable_value(&mut app.point_render_mode, PointRenderMode::Agents, "Sprites");
            ui.selectable_value(&mut app.point_render_mode, PointRenderMode::Density, "Density");

            // Edges are only drawn along with sprites, and only on a GPU that can build the edge layer.
            if let Some(directed) = edges_directed(app)
                && app.point_render_mode == PointRenderMode::Agents
                && app.runtime.vertex_storage
            {
                ui.separator();
                ui.checkbox(&mut app.show_edges, "Edges");
                if app.show_edges {
                    ui.add_enabled(directed, egui::Checkbox::new(&mut app.edge_arrows, "Arrows"))
                        .on_hover_text("Show direction with arrowheads")
                        .on_disabled_hover_text("Only directed edges have arrows");
                }
            }
        }
    });
    if app.point_render_mode != mode_before {
        // The snapshot is unchanged, but the mode drawing it has changed.
        app.last_rendered_serial = None;
    }

    draw_view(ui, app);

    app.timings.frame_render_ms = render_start.elapsed().as_secs_f64() * 1000.0;
}

fn draw_view(ui: &mut egui::Ui, app: &mut AppState) {
    // The snapshot is taken out for the duration, to keep it clear of borrows of the rest of `app`. Drawing needs
    // `app` for the textures and the agent pipeline.
    let Some(snapshot) = app.snapshot.take() else {
        ui.centered_and_justified(|ui| {
            ui.heading("Load a model to start simulation");
        });
        return;
    };
    draw_snapshot(ui, app, &snapshot);
    app.snapshot = Some(snapshot);
}

/// Draws `snapshot`, the snapshot taken out of `app` for the frame.
fn draw_snapshot(ui: &mut egui::Ui, app: &mut AppState, snapshot: &Snapshot) {
    if !app.rendering_enabled {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(MDI_CUBE_OFF_OUTLINE).size(64.0));
            ui.heading("Rendering disabled");
        });
        return;
    }

    // A GPU snapshot has nothing to convert or upload. The branch reads the snapshot variant, since a GPU Game of Life
    // is still `TopologyHint::GRID`.
    match (&snapshot.view, view_extent(&snapshot.view)) {
        (SnapshotView::Gpu(gpu), Some(extent)) => paint_gpu_view(ui, app, gpu, extent),
        (SnapshotView::Gpu(_), None) => {}
        (SnapshotView::Cpu(layers), Some(extent)) => paint_cpu_view(ui, app, layers, extent, snapshot.serial),
        (SnapshotView::Cpu(_), None) => {
            ui.label("No view available");
        }
    }
}

/// Copies `layers`, the layers of the snapshot numbered `serial`, to the GPU unless the viewport drew that snapshot
/// last, and paints them into one rect fitted to `extent`.
fn paint_cpu_view(ui: &mut egui::Ui, app: &mut AppState, layers: &CpuLayers, extent: ViewExtent, serial: u64) {
    let ctx = ui.ctx().clone();

    // Keyed on the serial. A layout relaxing while the simulation is paused moves positions without advancing the tick.
    if app.last_rendered_serial != Some(serial) {
        if let Some(grid) = &layers.grid {
            upload_grid(&ctx, app, grid);
        }
        if let Some(points) = &layers.points {
            match app.point_render_mode {
                PointRenderMode::Agents => upload_agents(app, points, layers.edges.as_deref()),
                PointRenderMode::Density => {
                    render_density_heatmap(&ctx, app, &points.pos_x, &points.pos_y, points.world_w, points.world_h);
                }
            }
        }
        app.last_rendered_serial = Some(serial);
    }

    // Both layers share one rect so they stay registered. See `PointView`'s docs.
    let display_size = fit_aspect(ui.available_size(), extent.width, extent.height);

    ui.centered_and_justified(|ui| {
        let (rect, _response) = ui.allocate_exact_size(display_size, egui::Sense::hover());

        // Painter order is the compositing order, field first and agents over the top.
        if let Some(texture) = &app.grid_texture {
            ui.painter().image(texture.id(), rect, UV_FULL, egui::Color32::WHITE);
        }
        if layers.points.is_some() {
            match app.point_render_mode {
                PointRenderMode::Agents => {
                    if let Some(layer) = &app.agent_layer {
                        layer.paint(ui, rect, extent.width, extent.height, app.edge_style());
                    }
                }
                PointRenderMode::Density => {
                    if let Some(texture) = &app.density_texture {
                        ui.painter().image(texture.id(), rect, UV_FULL, egui::Color32::WHITE);
                    }
                }
            }
        }
    });
}

const UV_FULL: egui::Rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));

/// Extent of a snapshot's view, which the viewport and an exported image fit their layers to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewExtent {
    /// Width of the field in cells, or of the agents' world when there is no field.
    pub width: f32,
    /// Height of the field in cells, or of the agents' world when there is no field.
    pub height: f32,
    /// Whether agents are drawn over the field.
    pub agents: bool,
}

/// Returns the extent the shared rect is fitted to, `None` for a view with neither a field nor agents. The agent
/// shader takes it as the world size.
///
/// A field takes precedence when present, since it is the layer with a fixed pixel shape.
pub fn view_extent(view: &SnapshotView) -> Option<ViewExtent> {
    let (field, world) = match view {
        SnapshotView::Cpu(layers) => (
            layers.grid.as_ref().map(|grid| (grid.width, grid.height)),
            layers.points.as_ref().map(|points| (points.world_w, points.world_h)),
        ),
        SnapshotView::Gpu(gpu) => (
            gpu.display.as_ref().map(|display| (display.width, display.height)),
            gpu.agents.as_ref().map(|agents| (agents.world_w, agents.world_h)),
        ),
    };
    let (width, height) = field.map(|(width, height)| (width as f32, height as f32)).or(world)?;
    Some(ViewExtent {
        width,
        height,
        agents: world.is_some(),
    })
}

/// Returns whether the snapshot's edges are directed, or `None` for a model without edges.
fn edges_directed(app: &AppState) -> Option<bool> {
    match app.snapshot.as_ref().map(|snapshot| &snapshot.view) {
        Some(SnapshotView::Cpu(layers)) => layers.edges.as_ref().map(|edges| edges.directed),
        _ => None,
    }
}

/// Returns whether the snapshot has points to draw, the one case the mode selector applies to.
fn has_points(app: &AppState) -> bool {
    matches!(
        app.snapshot.as_ref().map(|snapshot| &snapshot.view),
        Some(SnapshotView::Cpu(layers)) if layers.points.is_some()
    )
}

/// Returns the grid's colours at one texel per cell.
fn expand_grid(grid: &GridSnapshot) -> Vec<egui::Color32> {
    use rayon::prelude::*;
    let colors = grid_colors(&grid.palette);
    grid.cells.par_iter().map(|&cell| colors[usize::from(cell)]).collect()
}

/// Returns the colour of one representative cell per texel of a `width` x `height` texture, `colors` holding the colour
/// of every cell value. The viewport takes it for a grid too large to upload whole, and an exported image for a grid
/// of another size than the image.
pub fn sample_grid<T: Copy + Send + Sync>(grid: &GridSnapshot, width: u32, height: u32, colors: &[T; 256]) -> Vec<T> {
    use rayon::prelude::*;

    let grid_width = grid.width as usize;
    let row = |y: u32| {
        let source = source_row(y, grid.height, height) as usize;
        let cells = &grid.cells[source * grid_width..(source + 1) * grid_width];
        (0..width as usize)
            .map(|x| colors[usize::from(cells[x * grid_width / width as usize])])
            .collect::<Vec<_>>()
    };
    (0..height).into_par_iter().flat_map_iter(row).collect()
}

fn upload_grid(ctx: &egui::Context, app: &mut AppState, grid: &GridSnapshot) {
    // A CPU grid meets the texture limit too, through egui's upload.
    let device_max = app.render_ctx.device.limits().max_texture_dimension_2d;
    let (tex_w, tex_h) = display_dims(grid.width, grid.height, device_max);

    let pixels = app.ui_pool().install(|| {
        if (tex_w, tex_h) == (grid.width, grid.height) {
            expand_grid(grid)
        } else {
            sample_grid(grid, tex_w, tex_h, &grid_colors(&grid.palette))
        }
    });

    let image = ColorImage::new([tex_w as usize, tex_h as usize], pixels);

    match &mut app.grid_texture {
        Some(texture) => texture.set(image, TextureOptions::NEAREST),
        None => app.grid_texture = Some(ctx.load_texture("grid", image, TextureOptions::NEAREST)),
    }
}

/// Builds the agent layer on first use. Both backends share it.
fn ensure_agent_layer(app: &mut AppState) {
    if app.agent_layer.is_none() {
        app.agent_layer = Some(AgentLayer::new(
            &app.render_ctx.device,
            &app.render_ctx.queue,
            app.render_ctx.target_format,
            app.runtime.vertex_storage,
        ));
    }
}

fn upload_agents(app: &mut AppState, points: &PointSnapshot, edges: Option<&EdgeSnapshot>) {
    ensure_agent_layer(app);
    let pool = app.ui_pool().clone();
    if let Some(layer) = &mut app.agent_layer {
        layer.upload(points, &pool);
        layer.upload_edges(edges, &pool);
    }
}

const DENSITY_W: usize = 512;
const DENSITY_H: usize = 512;

/// Number of agents in one pixel of the density map at which its colour map saturates.
const DENSITY_SATURATION: f32 = 4.0;

/// Returns the colour of every cell value under `palette`, a value past its end taking the first colour.
///
/// Note that a cell value is the model's to choose. Indexing `palette` itself would panic on the UI thread for a
/// value past its end.
fn grid_colors(palette: &[[u8; 4]]) -> [egui::Color32; 256] {
    padded_palette(palette).map(|[r, g, b, a]| egui::Color32::from_rgba_unmultiplied(r, g, b, a))
}

/// Returns the colour at `t` of a five-stop piecewise linear approximation of the Inferno colour map.
#[inline]
fn inferno(t: f32) -> egui::Color32 {
    const STOPS: [[u8; 3]; 5] = [[0, 0, 4], [64, 4, 104], [183, 55, 121], [251, 136, 97], [252, 255, 164]];

    let t = t.clamp(0.0, 1.0);
    let scaled = t * 4.0;
    let segment = (scaled as usize).min(3);
    let fraction = scaled - segment as f32;

    let low = STOPS[segment];
    let high = STOPS[segment + 1];

    let red = low[0] as f32 + (high[0] as f32 - low[0] as f32) * fraction;
    let green = low[1] as f32 + (high[1] as f32 - low[1] as f32) * fraction;
    let blue = low[2] as f32 + (high[2] as f32 - low[2] as f32) * fraction;

    egui::Color32::from_rgb(red as u8, green as u8, blue as u8)
}

fn render_density_heatmap(
    ctx: &egui::Context,
    app: &mut AppState,
    pos_x: &[f32],
    pos_y: &[f32],
    world_w: f32,
    world_h: f32,
) {
    let density = app.ui_pool().install(|| density_counts(pos_x, pos_y, world_w, world_h));

    // An empty pixel is transparent, so a field underneath still shows through.
    let pixels: Vec<egui::Color32> = density
        .iter()
        .map(|&count| {
            if count == 0 {
                egui::Color32::TRANSPARENT
            } else {
                inferno(count as f32 / DENSITY_SATURATION)
            }
        })
        .collect();
    let image = ColorImage::new([DENSITY_W, DENSITY_H], pixels);

    match &mut app.density_texture {
        Some(texture) => {
            texture.set(image, TextureOptions::LINEAR);
        }
        None => {
            app.density_texture = Some(ctx.load_texture("density_heatmap", image, TextureOptions::LINEAR));
        }
    }
}

/// Returns the number of points in each pixel of the density map, row by row, for the points at `pos_x` and `pos_y`
/// in a `world_w` x `world_h` world. A point with a coordinate that is not finite is left out.
fn density_counts(pos_x: &[f32], pos_y: &[f32], world_w: f32, world_h: f32) -> Vec<u32> {
    use rayon::prelude::*;

    let num_pixels = DENSITY_W * DENSITY_H;
    let inv_world_w = DENSITY_W as f32 / world_w;
    let inv_world_h = DENSITY_H as f32 / world_h;

    let threads = rayon::current_num_threads().max(1);
    // At least one. An empty population would otherwise ask for chunks of zero, and `par_chunks` panics on those.
    let chunk_size = pos_x.len().div_ceil(threads).max(1);

    let partial: Vec<Vec<u32>> = pos_x
        .par_chunks(chunk_size)
        .zip(pos_y.par_chunks(chunk_size))
        .map(|(xs, ys)| {
            let mut counts = vec![0u32; num_pixels];
            for (&x, &y) in xs.iter().zip(ys.iter()) {
                if !(x.is_finite() && y.is_finite()) {
                    continue;
                }
                let px = ((x * inv_world_w) as usize).min(DENSITY_W - 1);
                let py = ((y * inv_world_h) as usize).min(DENSITY_H - 1);
                counts[py * DENSITY_W + px] += 1;
            }
            counts
        })
        .collect();

    let mut density = vec![0u32; num_pixels];
    for counts in &partial {
        for (total, &count) in density.iter_mut().zip(counts.iter()) {
            *total += count;
        }
    }
    density
}

#[cfg(test)]
mod tests {
    use henad_compute::snapshot::GridSnapshot;

    use super::{DENSITY_H, DENSITY_W, density_counts, expand_grid, grid_colors, sample_grid};

    /// A model's cell value can run past its palette. Indexing past its end would panic on the UI thread and end the
    /// app.
    #[test]
    fn a_cell_past_the_palette_takes_the_first_colour() {
        let grid = GridSnapshot {
            width: 4,
            height: 2,
            cells: vec![0, 1, 2, 255, 1, 1, 7, 0],
            palette: vec![[10, 20, 30, 255], [40, 50, 60, 255]],
        };
        let first = egui::Color32::from_rgb(10, 20, 30);
        let second = egui::Color32::from_rgb(40, 50, 60);
        assert_eq!(
            expand_grid(&grid),
            [first, second, first, first, second, second, first, first]
        );
        assert_eq!(sample_grid(&grid, 2, 1, &grid_colors(&grid.palette)), [first, first]);
    }

    /// A population can be empty. Splitting it into chunks of zero agents would panic on the UI thread and end the app.
    #[test]
    fn an_empty_population_draws_nothing() {
        let counts = density_counts(&[], &[], 100.0, 100.0);
        assert_eq!(counts.len(), DENSITY_W * DENSITY_H);
        assert!(counts.iter().all(|&count| count == 0));

        let one = density_counts(&[50.0], &[25.0], 100.0, 100.0);
        assert_eq!(one.iter().sum::<u32>(), 1, "one agent counts once");
        assert_eq!(one[DENSITY_H / 4 * DENSITY_W + DENSITY_W / 2], 1);
    }
}
