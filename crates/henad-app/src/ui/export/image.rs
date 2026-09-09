//! Capturing the viewport.

use henad_compute::display_scale::{MAX_DISPLAY_DIM, display_dims};
use henad_compute::gpu::view::display::GpuDisplay;
use henad_compute::snapshot::{CpuLayers, GpuSnapshot, GridSnapshot, SnapshotView};

use crate::state::AppState;
use crate::ui::agent_layer::AgentDraw;

/// A capture waiting on the GPU. Polled every frame, never blocking, the way a stats readback is.
pub struct PendingCapture {
    buffer: wgpu::Buffer,
    width: u32,
    height: u32,
    /// Rows in the staging buffer are padded to `COPY_BYTES_PER_ROW_ALIGNMENT`.
    padded_row: u32,
    format: wgpu::TextureFormat,
    mapped: flume::Receiver<Result<(), wgpu::BufferAsyncError>>,
    pub name: String,
}

/// Long side an export carrying agents is grown to, a whole number of times over the field.
///
/// A field is coarse next to the population over it. Ants lay pheromone on 200 cells a side and
/// put 50 000 agents inside them, so a field-sized image is one dense mass with every sub-cell
/// position rounded away.
const MIN_AGENT_DIM: u32 = 1000;

/// Pixel dimensions an export of this snapshot gets.
///
/// Returns `None` if there is nothing to draw.
pub fn capture_dims(app: &AppState, device_max: u32) -> Option<(u32, u32)> {
    let view = &app.snapshot.as_ref()?.view;
    let (base, agents) = match view {
        SnapshotView::Cpu(layers) => match layers {
            CpuLayers {
                grid: Some(grid),
                points,
            } => ((grid.width, grid.height), points.is_some()),
            CpuLayers {
                points: Some(points), ..
            } => ((points.world_w as u32, points.world_h as u32), true),
            CpuLayers { .. } => return None,
        },
        SnapshotView::Gpu(gpu) => match gpu {
            GpuSnapshot {
                display: Some(display),
                agents,
            } => ((display.width, display.height), agents.is_some()),
            GpuSnapshot {
                agents: Some(agents), ..
            } => ((agents.world_w as u32, agents.world_h as u32), true),
            GpuSnapshot { .. } => return None,
        },
    };

    let (width, height) = display_dims(base.0.max(1), base.1.max(1), device_max);
    if !agents {
        return Some((width, height));
    }
    let cap = MAX_DISPLAY_DIM.min(device_max).max(1);
    let scale = agent_scale(width.max(height), cap);
    Some((width * scale, height * scale))
}

/// Whole-number scale taking `long_side` to [`MIN_AGENT_DIM`] without passing `cap`.
fn agent_scale(long_side: u32, cap: u32) -> u32 {
    let long_side = long_side.max(1);
    let wanted = MIN_AGENT_DIM.div_ceil(long_side).max(1);
    let allowed = (cap / long_side).max(1);
    wanted.min(allowed)
}

/// Draw the layers into an offscreen target and start reading it back.
///
/// # Errors
/// If the snapshot has no layer to draw.
pub fn start(app: &AppState, name: String) -> Result<PendingCapture, String> {
    let ctx = &app.render_ctx;
    let device_max = ctx.device.limits().max_texture_dimension_2d;
    let (width, height) = capture_dims(app, device_max).ok_or("model exposes no view to capture")?;
    let format = ctx.target_format;

    let target = make_target(&ctx.device, format, width, height);
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let snapshot = app.snapshot.as_ref().ok_or("no snapshot")?;

    // A CPU grid has no pipeline of its own, the panel uploads it as a texture. Writing the cells
    // straight into the target is the same picture with nothing sampling it.
    if let SnapshotView::Cpu(layers) = &snapshot.view
        && let Some(grid) = &layers.grid
    {
        write_grid(ctx, &target, grid, width, height, format);
    }

    let gpu_display = match &snapshot.view {
        SnapshotView::Gpu(gpu) => gpu.display.clone(),
        SnapshotView::Cpu(_) => None,
    };
    // Prepared before the encoder, since it writes the sprite uniform through the queue.
    let agents = agent_draw(app, width, height);

    let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("henad_export_capture"),
    });
    draw_layers(&mut encoder, &view, gpu_display.as_deref(), agents.as_ref());

    let padded_row = padded_row(width);
    let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("henad_export_staging"),
        size: u64::from(padded_row) * u64::from(height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        copy_source(&target),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(height),
            },
        },
        extent(width, height),
    );
    ctx.queue.submit(Some(encoder.finish()));

    let (tx, rx) = flume::bounded(1);
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
        drop(tx.send(result));
    });

    Ok(PendingCapture {
        buffer,
        width,
        height,
        padded_row,
        format,
        mapped: rx,
        name,
    })
}

fn make_target(device: &wgpu::Device, format: wgpu::TextureFormat, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("henad_export_target"),
        size: extent(width, height),
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_grid(
    ctx: &henad_compute::gpu::GpuContext,
    target: &wgpu::Texture,
    grid: &GridSnapshot,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) {
    let pixels = grid_pixels(grid, width, height, format);
    ctx.queue.write_texture(
        copy_source(target),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        extent(width, height),
    );
}

/// The field first and agents over the top, the same order the panel composites in.
fn draw_layers(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    display: Option<&GpuDisplay>,
    agents: Option<&AgentDraw>,
) {
    // A CPU grid is already in the target, so keep it. Everything else starts from nothing.
    let load = if display.is_some() {
        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)
    } else {
        wgpu::LoadOp::Load
    };
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("henad_export_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });

    if let Some(display) = display {
        pass.set_pipeline(&display.render_pipeline);
        pass.set_bind_group(0, &display.render_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    if let Some(draw) = agents {
        draw.record(&mut pass);
    }
}

fn copy_source(texture: &wgpu::Texture) -> wgpu::TexelCopyTextureInfo<'_> {
    wgpu::TexelCopyTextureInfo {
        texture,
        mip_level: 0,
        origin: wgpu::Origin3d::ZERO,
        aspect: wgpu::TextureAspect::All,
    }
}

fn extent(width: u32, height: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    }
}

impl PendingCapture {
    /// The PNG once the GPU is done, `None` while it is not. Never blocks, so this can run every
    /// frame on the web, where blocking on the main thread is not allowed.
    ///
    /// # Errors
    /// If the mapping fails, or the PNG cannot be encoded.
    pub fn poll(&self, device: &wgpu::Device) -> Option<Result<Vec<u8>, String>> {
        device.poll(wgpu::PollType::Poll).ok();
        match self.mapped.try_recv() {
            Ok(Ok(())) => Some(self.encode()),
            Ok(Err(err)) => Some(Err(err.to_string())),
            Err(_) => None,
        }
    }

    fn encode(&self) -> Result<Vec<u8>, String> {
        let rgba = {
            let mapped = self
                .buffer
                .slice(..)
                .get_mapped_range()
                .map_err(|err| err.to_string())?;
            let row = self.width as usize * 4;
            let mut rgba = Vec::with_capacity(row * self.height as usize);
            for y in 0..self.height as usize {
                let start = y * self.padded_row as usize;
                rgba.extend_from_slice(&mapped[start..start + row]);
            }
            match_channel_order(&mut rgba, self.format);
            rgba
        };
        self.buffer.unmap();

        let mut png = Vec::new();
        image::write_buffer_with_format(
            &mut std::io::Cursor::new(&mut png),
            &rgba,
            self.width,
            self.height,
            image::ExtendedColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .map_err(|err| err.to_string())?;
        Ok(png)
    }
}

/// The population as a draw into a `width` x `height` target, or `None` when there is none.
fn agent_draw(app: &AppState, width: u32, height: u32) -> Option<AgentDraw> {
    let target = egui::vec2(width as f32, height as f32);
    let layer = app.agent_layer.as_ref()?;
    let snapshot = app.snapshot.as_ref()?;
    match &snapshot.view {
        SnapshotView::Cpu(layers) => layer.offscreen_draw(target, layers.points.as_ref(), None),
        SnapshotView::Gpu(gpu) => layer.offscreen_draw(target, None, gpu.agents.as_deref()),
    }
}

/// The grid as target-format bytes, one pixel per cell, sampled the way the panel's texture is
/// once the grid is past the cap.
fn grid_pixels(grid: &GridSnapshot, width: u32, height: u32, format: wgpu::TextureFormat) -> Vec<u8> {
    use rayon::prelude::*;

    let src_width = grid.width as usize;
    let row = |y: u32| {
        let sy = henad_compute::display_scale::source_row(y, grid.height, height) as usize;
        let cells = &grid.cells[sy * src_width..(sy + 1) * src_width];
        (0..width as usize)
            .flat_map(|x| {
                let cell = cells[x * src_width / width as usize];
                grid.palette[cell as usize]
            })
            .collect::<Vec<u8>>()
    };
    let mut pixels: Vec<u8> = (0..height).into_par_iter().flat_map_iter(row).collect();
    match_channel_order(&mut pixels, format);
    pixels
}

/// Rows a texture copy needs, padded to `COPY_BYTES_PER_ROW_ALIGNMENT`.
fn padded_row(width: u32) -> u32 {
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    width * 4 + (align - (width * 4) % align) % align
}

/// Swap red and blue where the surface is BGRA. Its own inverse, so the upload into the target and
/// the readback out of it both go through this.
fn match_channel_order(pixels: &mut [u8], format: wgpu::TextureFormat) {
    if !matches!(
        format,
        wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
    ) {
        return;
    }
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
}

#[cfg(test)]
mod tests {
    use super::{MIN_AGENT_DIM, agent_scale, padded_row};
    use henad_compute::display_scale::MAX_DISPLAY_DIM;

    #[test]
    fn a_row_is_padded_to_the_copy_alignment() {
        assert_eq!(padded_row(64), 256, "already aligned");
        assert_eq!(padded_row(1), 256, "4 bytes rounds up to one block");
        assert_eq!(padded_row(65), 512);
        assert_eq!(padded_row(1024), 4096);
    }

    /// Ants: 200 cells a side, so five pixels a cell and the agents get somewhere to land.
    #[test]
    fn a_coarse_field_is_grown_a_whole_number_of_times() {
        assert_eq!(agent_scale(200, MAX_DISPLAY_DIM), 5);
        assert_eq!(200 * agent_scale(200, MAX_DISPLAY_DIM), 1000);
        assert_eq!(agent_scale(3, MAX_DISPLAY_DIM), MIN_AGENT_DIM.div_ceil(3));
    }

    /// Boids already covers a thousand world units, and a large field is left alone.
    #[test]
    fn a_field_already_big_enough_is_left_as_it_is() {
        assert_eq!(agent_scale(1000, MAX_DISPLAY_DIM), 1);
        assert_eq!(agent_scale(4096, MAX_DISPLAY_DIM), 1);
    }

    /// Growing past the texture cap would refuse to allocate.
    #[test]
    fn the_cap_wins_over_the_minimum() {
        assert_eq!(agent_scale(700, 1024), 1, "2x would be 1400, past a 1024 cap");
        assert_eq!(agent_scale(200, 512), 2);
        assert_eq!(agent_scale(4096, 2048), 1, "never scales below one");
    }
}
