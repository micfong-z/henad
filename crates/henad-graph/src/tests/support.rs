//! Helpers the tests of this crate share: the headless device and the set-up of a device test, a runner of one generated
//! shader on it, the graphs the graph tests build, run and read, the random edits of the mutation corpora, and the graph
//! text of the files past the device limits. The sheet helpers sit in [`super::sheet_support`].
//!
//! henad-graph sits below henad-explore's testing kit and keeps its own device, as henad-compute does.

use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::fault::FaultSink;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::gpu::wgpu;
use henad_compute::gpu::{BufferSide, GpuContext, ShaderBinding};
use henad_core::authoring::model::binding::{BindingKind, named_buffer_target};
use henad_core::authoring::primitives::rng::xorshift64;
use henad_core::authoring::primitives::space::Boundary;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::params::ParamValue;

use crate::GraphFile;
use crate::document::PassStage;
use crate::edit::GraphBuilder;
use crate::host::{HostEvaluator, HostLanes, HostWorld, RunContext};
use crate::ids::NodeId;
use crate::value::{InputValue, PortRef};
use crate::wgsl::{Compiled, GeneratedShader, Probes};

// The headless device and the set-up of a device test.

/// Set in CI to turn "no GPU here" from a skip into a failure.
const REQUIRE_GPU: &str = "HENAD_REQUIRE_GPU";

/// A device and its queue.
pub(super) struct TestDevice {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Returns a headless device, or `None` when this machine has none and `HENAD_REQUIRE_GPU` is unset.
///
/// # Panics
///
/// Panics if `HENAD_REQUIRE_GPU` is set and no device could be acquired.
pub(super) fn test_device() -> Option<TestDevice> {
    // A case of a radius fold binds its output lanes, the index and the counters, past the baseline's 8 storage
    // buffers, as a model raises the device for its widest pass.
    device_with(|available| wgpu::Limits {
        max_storage_buffers_per_shader_stage: available.max_storage_buffers_per_shader_stage.min(16),
        ..wgpu::Limits::default()
    })
}

/// Returns a headless device with the WebGPU default limits, or `None` when this machine has none and
/// `HENAD_REQUIRE_GPU` is unset.
///
/// # Panics
///
/// Panics if `HENAD_REQUIRE_GPU` is set and no device could be acquired.
pub(super) fn baseline_test_device() -> Option<TestDevice> {
    device_with(|_| wgpu::Limits::default())
}

/// Returns a headless device with the limits `limits` returns from the adapter's, or `None` when this machine has none
/// and `HENAD_REQUIRE_GPU` is unset.
fn device_with(limits: impl FnOnce(&wgpu::Limits) -> wgpu::Limits) -> Option<TestDevice> {
    let required = std::env::var_os(REQUIRE_GPU).is_some_and(|value| !value.is_empty() && value != "0");
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
    let adapter = match adapter {
        Ok(adapter) => adapter,
        Err(error) => {
            assert!(!required, "{REQUIRE_GPU} is set but no adapter is available: {error}");
            return None;
        }
    };
    let required_limits = limits(&adapter.limits());
    match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("henad_graph_tests"),
        required_limits,
        ..Default::default()
    })) {
        Ok((device, queue)) => Some(TestDevice { device, queue }),
        Err(error) => {
            assert!(
                !required,
                "{REQUIRE_GPU} is set but the adapter gave no device: {error}"
            );
            None
        }
    }
}

/// Prints that the test `test` skips its device half, since this machine has no adapter.
#[expect(
    clippy::print_stderr,
    reason = "the skip is information for a reader of the test output"
)]
pub(super) fn note_skip(test: &str) {
    eprintln!("skipping {test}: no adapter");
}

/// Returns the test device, or `None` with a note naming the skipped test `test` when this machine has none.
pub(super) fn device_for(test: &str) -> Option<TestDevice> {
    let device = test_device();
    if device.is_none() {
        note_skip(test);
    }
    device
}

/// Returns a context over `device` that draws into an `Rgba8Unorm` texture.
pub(super) fn context(device: TestDevice) -> GpuContext {
    GpuContext::new(
        device.device,
        device.queue,
        wgpu::TextureFormat::Rgba8Unorm,
        FaultSink::new(),
    )
}

/// Returns the parameters of `entry` at their defaults, with each of `set` in place of its default.
pub(super) fn params_of(entry: &ModelEntry, set: &[(&str, ParamValue)]) -> Vec<ParamValue> {
    entry
        .param_descriptors()
        .iter()
        .map(|descriptor| {
            set.iter()
                .find(|(id, _)| *id == descriptor.id())
                .map_or_else(|| descriptor.kind().default_value(), |(_, value)| value.clone())
        })
        .collect()
}

/// Builds `entry` from `params` at `seed` on `ctx`, which must give a state on the device.
pub(super) fn built(
    entry: &ModelEntry,
    params: &[ParamValue],
    seed: Option<u64>,
    ctx: &GpuContext,
) -> Box<dyn GpuSimState> {
    match entry.build(params, seed, Some(ctx)) {
        Ok(ModelState::Gpu(state)) => state,
        Ok(ModelState::Cpu(_)) => panic!("{} built on the CPU", entry.id()),
        Err(fault) => panic!("{} did not build: {fault}", entry.id()),
    }
}

// One run of a generated shader on the device.

/// Returns the words and labels of each lane buffer of `compiled`, built from `lanes`, then the tick buffer holding
/// `tick` when `compiled` has one.
///
/// A packed buffer holds the segments of its lanes one after another, as the engine uploads it.
pub(super) fn lane_buffers(compiled: &Compiled, lanes: &HostLanes, tick: u32) -> (Vec<Vec<u32>>, Vec<String>) {
    let mut words: Vec<Vec<u32>> = compiled
        .layout
        .groups
        .iter()
        .map(|group| {
            group
                .lanes
                .iter()
                .flat_map(|&lane| lanes.columns()[lane].words().iter().copied())
                .collect()
        })
        .collect();
    let mut labels: Vec<String> = compiled.layout.groups.iter().map(|group| group.label.clone()).collect();
    if compiled.tick_buffer {
        words.push(vec![tick]);
        labels.push("henad_tick".to_owned());
    }
    (words, labels)
}

/// Returns the words of the lane at `position` from `buffers`, the buffers that a run of one of the shaders of
/// `compiled` left.
///
/// # Panics
///
/// Panics if no buffer holds the lane.
pub(super) fn lane_words(compiled: &Compiled, buffers: &[Vec<u32>], position: usize) -> Vec<u32> {
    let place = compiled.layout.place(position).expect("a lane on the device");
    place.segment(&buffers[place.buffer()]).to_vec()
}

/// Returns the shader among `shaders` that runs the pass `pass`.
pub(super) fn shader_of_pass<'a>(shaders: &'a [GeneratedShader], pass: &str) -> Option<&'a GeneratedShader> {
    shaders
        .iter()
        .find(|shader| shader.passes().iter().any(|known| known == pass))
}

/// Returns the position of the first tally of the stats pass `pass` among the counters of `shader`, after the tallies
/// of the passes `shader` runs before it.
pub(super) fn first_tally(evaluator: &HostEvaluator, shader: &GeneratedShader, pass: &str) -> usize {
    shader
        .passes()
        .iter()
        .take_while(|known| *known != pass)
        .map(|known| evaluator.program(known).expect("a stats pass").1.tallies)
        .sum()
}

/// Buffers and sizes one run of a shader binds.
pub(super) struct ShaderRun<'a> {
    /// Columns of the grid.
    pub width: u32,
    /// Rows of the grid.
    pub height: u32,
    /// Words of each buffer, lanes then the tick buffer, as both sides start.
    pub buffers: &'a [Vec<u32>],
    /// Label of each buffer, in the same order.
    pub labels: &'a [String],
    /// Uniform block bound as `params`.
    pub uniform: &'a [u8],
    /// Shape of a run of a shader of an agents world, `None` for a grid's.
    pub agents: Option<AgentShape<'a>>,
}

/// Dispatch and resources of a run of a shader of an agents world.
pub(super) struct AgentShape<'a> {
    /// Workgroups the run dispatches.
    pub groups: (u32, u32),
    /// Size of the display texture.
    pub texture: (u32, u32),
    /// Words of each resource the engine owns that the shader may bind, by binding name, such as `counters`,
    /// `partials` or `cell_start`.
    pub named: &'a [(&'a str, Vec<u32>)],
}

/// What one run of a shader leaves.
pub(super) struct ShaderResult {
    /// Words of each buffer after the run: the side the shader wrote, or the current side.
    pub buffers: Vec<Vec<u32>>,
    /// Counters of a reduce shader.
    pub counters: Vec<u32>,
    /// RGBA of each texel of a display shader, in row-major order.
    pub texels: Vec<[u8; 4]>,
    /// Words of each named resource of an agents run after the run, in the order of [`AgentShape::named`].
    pub named: Vec<Vec<u32>>,
}

/// Returns the layout entry of `binding` at `index`.
fn layout_entry(index: u32, binding: &ShaderBinding) -> wgpu::BindGroupLayoutEntry {
    let ty = match binding.kind {
        BindingKind::Storage { read_only } => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        BindingKind::Uniform => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        BindingKind::StorageTexture => wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: wgpu::TextureFormat::Rgba8Unorm,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
    };
    wgpu::BindGroupLayoutEntry {
        binding: index,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty,
        count: None,
    }
}

/// Returns a storage buffer holding `words`, at least one word long.
fn storage(device: &TestDevice, label: &str, words: &[u32]) -> wgpu::Buffer {
    let mut padded = words.to_vec();
    if padded.is_empty() {
        padded.push(0);
    }
    let buffer = device.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (padded.len() * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    device.queue.write_buffer(
        &buffer,
        0,
        &padded.iter().flat_map(|word| word.to_le_bytes()).collect::<Vec<_>>(),
    );
    buffer
}

/// Returns the bytes of `buffer`, copied out and mapped.
fn read_bytes(device: &TestDevice, buffer: &wgpu::Buffer) -> Vec<u8> {
    let staging = device.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("henad_graph_test_staging"),
        size: buffer.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
    device.queue.submit(Some(encoder.finish()));
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the device polls");
    let bytes = slice.get_mapped_range().expect("the mapped range").to_vec();
    staging.unmap();
    bytes
}

/// Returns the words of `buffer`.
fn read_words(device: &TestDevice, buffer: &wgpu::Buffer) -> Vec<u32> {
    read_bytes(device, buffer)
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
        .collect()
}

/// Runs `shader` once over `run`, as the engine binds it, and returns what it leaves.
///
/// # Panics
///
/// Panics on a validation error of the shader or its bindings.
#[expect(
    clippy::too_many_lines,
    reason = "the bindings, the dispatch and the readbacks of one run read in order"
)]
pub(super) fn run_shader(device: &TestDevice, shader: &GeneratedShader, run: &ShaderRun<'_>) -> ShaderResult {
    let scope = device.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let sides: Vec<(wgpu::Buffer, wgpu::Buffer)> = run
        .buffers
        .iter()
        .zip(run.labels)
        .map(|(words, label)| (storage(device, label, words), storage(device, label, words)))
        .collect();
    let uniform = {
        let buffer = device.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: run.uniform.len().max(16) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        device.queue.write_buffer(&buffer, 0, run.uniform);
        buffer
    };
    let dims = {
        let buffer = device.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dims"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let words = [run.width, run.height, run.width, run.height];
        device.queue.write_buffer(
            &buffer,
            0,
            &words.iter().flat_map(|word| word.to_le_bytes()).collect::<Vec<_>>(),
        );
        buffer
    };
    let counters = storage(device, "counters", &vec![0; shader.counters()]);
    let named: Vec<(&str, wgpu::Buffer)> = run
        .agents
        .as_ref()
        .map(|shape| {
            shape
                .named
                .iter()
                .map(|(name, words)| (*name, storage(device, name, words)))
                .collect()
        })
        .unwrap_or_default();
    let (texture_width, texture_height) = run
        .agents
        .as_ref()
        .map_or((run.width, run.height), |shape| shape.texture);
    let texture = device.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("output"),
        size: wgpu::Extent3d {
            width: texture_width,
            height: texture_height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let rows = shader.sides().rows();
    let side = |binding: &ShaderBinding| -> Option<(usize, BufferSide)> {
        let (label, writes) = named_buffer_target(&binding.name, binding.kind)?;
        let index = run.labels.iter().position(|known| known == label)?;
        let row = rows[index];
        Some((index, if writes { row.write } else { row.read }))
    };
    let entries: Vec<wgpu::BindGroupLayoutEntry> = shader
        .bindings()
        .iter()
        .enumerate()
        .map(|(index, binding)| layout_entry(index as u32, binding))
        .collect();
    let layout = device
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(shader.label()),
            entries: &entries,
        });
    let group_entries: Vec<wgpu::BindGroupEntry<'_>> = shader
        .bindings()
        .iter()
        .enumerate()
        .map(|(index, binding)| {
            let resource = if let Some((buffer, side)) = side(binding) {
                match side {
                    BufferSide::Next => sides[buffer].1.as_entire_binding(),
                    _ => sides[buffer].0.as_entire_binding(),
                }
            } else if let Some((_, buffer)) = named.iter().find(|(name, _)| *name == binding.name.as_ref()) {
                buffer.as_entire_binding()
            } else {
                match binding.name.as_ref() {
                    "params" => uniform.as_entire_binding(),
                    "dims" => dims.as_entire_binding(),
                    "counters" => counters.as_entire_binding(),
                    "output" => wgpu::BindingResource::TextureView(&view),
                    other => panic!("{}: no resource for `{other}`", shader.label()),
                }
            };
            wgpu::BindGroupEntry {
                binding: index as u32,
                resource,
            }
        })
        .collect();
    let group = device.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(shader.label()),
        layout: &layout,
        entries: &group_entries,
    });
    let module = device.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(shader.label()),
        source: wgpu::ShaderSource::Wgsl(shader.composed().into()),
    });
    let pipeline_layout = device.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(shader.label()),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(shader.label()),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    let mut encoder = device
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        let (x, y) = run
            .agents
            .as_ref()
            .map_or((run.width.div_ceil(16), run.height.div_ceil(16)), |shape| shape.groups);
        pass.dispatch_workgroups(x, y, 1);
    }
    let bytes_per_row = (texture_width * 4).next_multiple_of(256);
    let texels = device.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("texels"),
        size: u64::from(bytes_per_row * texture_height),
        usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &texels,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: texture_width,
            height: texture_height,
            depth_or_array_layers: 1,
        },
    );
    device.queue.submit(Some(encoder.finish()));
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("{}: {error}\n{}", shader.label(), shader.text());
    }
    let buffers = sides
        .iter()
        .enumerate()
        .map(|(index, (current, next))| {
            let written = shader
                .bindings()
                .iter()
                .filter_map(&side)
                .any(|(buffer, side)| buffer == index && side == BufferSide::Next);
            read_words(device, if written { next } else { current })
        })
        .collect();
    let texel_bytes = read_bytes(device, &texels);
    let texels = (0..texture_height as usize)
        .flat_map(|y| (0..texture_width as usize).map(move |x| (y, x)))
        .map(|(y, x)| {
            let start = y * bytes_per_row as usize + x * 4;
            [
                texel_bytes[start],
                texel_bytes[start + 1],
                texel_bytes[start + 2],
                texel_bytes[start + 3],
            ]
        })
        .collect();
    ShaderResult {
        buffers,
        counters: read_words(device, &counters),
        texels,
        named: named.iter().map(|(_, buffer)| read_words(device, buffer)).collect(),
    }
}

// Graphs the graph tests build, run and read.

/// Returns node `number`'s id.
pub(super) fn node_id(number: u32) -> NodeId {
    NodeId::new(number).expect("node ids count from 1")
}

/// Returns a reference to output `port` of `node`, or to its only output when `port` is `None`.
pub(super) fn link(node: NodeId, port: Option<&str>) -> InputValue {
    InputValue::Link(PortRef {
        node,
        port: port.map(str::to_owned),
    })
}

/// Returns a builder of the starter `name`, with keys drawn from `seed`.
pub(super) fn starter_builder(name: &str, seed: u64) -> GraphBuilder {
    let text = crate::starters::starter(name).expect("a starter").text;
    GraphBuilder::from_file(&GraphFile::parse(text).expect("the starter reads")).with_key_seed(seed)
}

/// Returns a builder of `text` read through its canonical text, where every link to an only output is bare, with keys
/// drawn from `seed`.
pub(super) fn canonical_builder(text: &str, seed: u64) -> GraphBuilder {
    let canonical = GraphFile::parse(text).expect("the text reads").to_toml();
    GraphBuilder::from_file(&GraphFile::parse(&canonical).expect("the canonical text reads")).with_key_seed(seed)
}

/// Asserts that the builder's graph reads back and checks without an error, and returns the file.
#[track_caller]
pub(super) fn assert_checks(builder: &GraphBuilder) -> GraphFile {
    let file = builder.to_file().expect("the graph reads back");
    let errors: Vec<String> = file.check().errors().map(|error| error.message.clone()).collect();
    assert!(errors.is_empty(), "{errors:#?}\n{}", builder.to_toml());
    file
}

/// Returns the words of every lane of the SIR graph `file` at 24 by 16 after its init from seed 11 and `ticks` runs of
/// its pass `step`.
pub(super) fn sir_words_after(file: &GraphFile, ticks: u32) -> Vec<Vec<u32>> {
    let evaluator = HostEvaluator::new(file).expect("the evaluator");
    let mut lanes = evaluator.init(torus(24, 16), &[], Some(11)).expect("the init runs");
    for tick in 0..ticks {
        evaluator
            .run_pass("step", &mut lanes, &[], &RunContext::tick(Some(11), tick), &mut 1)
            .expect("the step runs");
    }
    lanes.columns().iter().map(|column| column.words().to_vec()).collect()
}

/// Returns the messages of the checker's errors of the graph `text`, which must read.
pub(super) fn graph_errors(text: &str) -> Vec<String> {
    GraphFile::parse(text)
        .unwrap_or_else(|error| panic!("the file reads: {error}\n{text}"))
        .check()
        .errors()
        .map(|diagnostic| diagnostic.message.clone())
        .collect()
}

/// Returns the reader's diagnostic of the graph `text`, which the reader must refuse.
pub(super) fn graph_refusal(text: &str) -> CodeDiagnostic {
    GraphFile::parse(text)
        .expect_err("the reader refuses the text")
        .diagnostic()
        .clone()
}

/// Returns the line of `text` that holds the first `needle`, counted from 1.
pub(super) fn line_of(text: &str, needle: &str) -> u32 {
    let at = text
        .find(needle)
        .unwrap_or_else(|| panic!("the text holds no {needle:?}"));
    u32::try_from(text[..at].matches('\n').count() + 1).expect("a short text")
}

/// Returns the wrapping grid of `width` by `height` cells.
pub(super) const fn torus(width: u32, height: u32) -> HostWorld {
    HostWorld::Grid {
        width,
        height,
        boundary: Boundary::Torus,
    }
}

/// Runs one step of the tick passes of `evaluator` on the host at `tick`, and returns the probes' counts.
pub(super) fn host_step(
    evaluator: &HostEvaluator,
    lanes: &mut HostLanes,
    params: &[ParamValue],
    seed: Option<u64>,
    tick: u64,
    probes: &Probes,
) -> Vec<u32> {
    let document = evaluator.document();
    let values = evaluator.param_values(params);
    let mut counts = vec![0; probes.len()];
    let mut stream = 0;
    for (position, pass) in document.passes.iter().enumerate() {
        if pass.stage != PassStage::Tick || !evaluator.is_enabled(position, &values) {
            continue;
        }
        evaluator
            .run_pass_counting(
                &pass.id,
                lanes,
                params,
                &RunContext::tick(seed, tick as u32),
                &mut stream,
                probes,
                &mut counts,
            )
            .unwrap_or_else(|error| panic!("{}: {error}", pass.id));
    }
    counts
}

// Random edits of the mutation corpora.

/// Values and lines that the random edits of a mutation corpus put into a text.
pub(super) struct HostileText {
    /// Values a token edit puts in place of a value.
    pub values: &'static [&'static str],
    /// Lines an edit inserts.
    pub lines: &'static [&'static str],
}

/// Returns a number below `bound` from `rng`.
pub(super) fn below(rng: &mut u64, bound: usize) -> usize {
    *rng = xorshift64(*rng);
    (*rng >> 33) as usize % bound.max(1)
}

/// Returns `text` with one random edit: a line removed, copied or swapped, a value replaced by one of
/// `hostile.values`, a line of `hostile.lines` inserted, or the text cut short.
fn mutate(text: &str, rng: &mut u64, hostile: &HostileText) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    if lines.is_empty() {
        return String::new();
    }
    let at = below(rng, lines.len());
    match below(rng, 7) {
        0 => {
            lines.remove(at);
        }
        1 => {
            let copy = lines[at].clone();
            lines.insert(below(rng, lines.len()), copy);
        }
        2 => {
            let other = below(rng, lines.len());
            lines.swap(at, other);
        }
        3 | 4 => {
            if let Some((key, _)) = lines[at].split_once(" = ") {
                let value = hostile.values[below(rng, hostile.values.len())];
                lines[at] = format!("{key} = {value}");
            }
        }
        5 => lines.insert(at, hostile.lines[below(rng, hostile.lines.len())].to_owned()),
        _ => {
            let joined = lines.join("\n");
            let mut cut = below(rng, joined.len());
            while !joined.is_char_boundary(cut) {
                cut -= 1;
            }
            return joined[..cut].to_owned();
        }
    }
    lines.join("\n")
}

/// Returns `count` random edits of `text` drawn from `rng` by [`mutate`], where every third edit is edited once more.
pub(super) fn mutations<'a>(
    text: &'a str,
    rng: &'a mut u64,
    hostile: &'a HostileText,
    count: usize,
) -> impl Iterator<Item = String> + 'a {
    (0..count).map(move |edit| {
        let edited = mutate(text, rng, hostile);
        if edit % 3 == 0 {
            mutate(&edited, rng, hostile)
        } else {
            edited
        }
    })
}

/// Returns `text` once for each line that holds a key, with that line deleted.
pub(super) fn without_each_key(text: &str) -> impl Iterator<Item = String> + '_ {
    text.lines()
        .enumerate()
        .filter(|(_, line)| line.contains(" = "))
        .map(move |(index, _)| {
            text.lines()
                .enumerate()
                .filter(|(other, _)| *other != index)
                .map(|(_, line)| line)
                .collect::<Vec<_>>()
                .join("\n")
        })
}

// Graph text of the files past the device limits.

/// Returns a node `id` of kind `op` with the further lines `lines`.
pub(super) fn node(id: usize, op: &str, lines: &str) -> String {
    format!("\n[[pass.node]]\nid = \"n{id}\"\nop = \"{op}\"\n{lines}")
}

/// Returns the table of a category `big` of 256 variants.
pub(super) fn big_category() -> String {
    let variants: Vec<String> = (0..256)
        .map(|k| format!("{{ name = \"V{k}\", label = \"V{k}\", color = [{k}, 0, 0, 255] }}"))
        .collect();
    format!("\n[[category]]\nid = \"big\"\nvariants = [{}]\n", variants.join(", "))
}

/// Returns a stats pass over `domain` that counts the lane `c`, of the category [`big_category`], with `counts` nodes,
/// 1 KiB of tallies each.
pub(super) fn tally_pass(domain: &str, counts: usize) -> String {
    let mut pass = format!("\n[[pass]]\nid = \"counts\"\nstage = \"stats\"\nkind = \"map\"\ndomain = \"{domain}\"\n");
    pass.push_str(&node(1, "lane.get", "config.lane = \"c\"\n"));
    for k in 0..counts {
        pass.push_str(&node(
            k + 2,
            "stat.count_by",
            "config.category = \"big\"\nin.value = \"n1\"\n",
        ));
    }
    pass
}

/// Returns the lane `x` over `domain` and a tick pass `step` that sums `powers` integer powers of 64 entries' tables
/// each into it.
pub(super) fn powi_flood_tables(domain: &str, powers: usize) -> (String, String) {
    let declarations =
        format!("\n[[lane]]\nid = \"x\"\ndomain = \"{domain}\"\ntype = \"f32\"\ninitial = {{ f32 = 0.0 }}\n");
    let mut passes = format!("\n[[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"{domain}\"\n");
    passes.push_str(&node(1, "ctx.index", ""));
    let mut sum: Option<usize> = None;
    let mut next = 2;
    for k in 0..powers {
        let power = next;
        let base = 1.0 + k as f32 / 1000.0;
        passes.push_str(&node(
            power,
            "math.powi",
            &format!("config.max_exponent = 64\nin.base = {{ f32 = {base:?} }}\nin.exponent = \"n1\"\n"),
        ));
        next += 1;
        sum = Some(match sum {
            None => power,
            Some(previous) => {
                passes.push_str(&node(
                    next,
                    "math.add",
                    &format!("in.a = \"n{previous}\"\nin.b = \"n{power}\"\n"),
                ));
                next += 1;
                next - 1
            }
        });
    }
    let value = sum.unwrap_or(1);
    passes.push_str(&node(
        next,
        "lane.set",
        &format!("config.lane = \"x\"\nin.value = \"n{value}\"\n"),
    ));
    (declarations, passes)
}
