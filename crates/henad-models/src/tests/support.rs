//! Helpers the test modules of henad-models share: the entries model files build, a GPU state stepped, pressed and
//! read back, the parameters a test builds at, and the fixtures and device limits the tests read.

use henad_compute::entry::{EntryDeclarations, ModelEntry, ModelSet, ModelState};
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::gpu::stepping::sample_stats;
use henad_core::metadata::Structure;
use henad_core::params::ParamValue;
use henad_core::topology::TopologyHint;
use henad_core::view::{StatEntry, StatValue};
use henad_explore::model_file::ModelFile;
use henad_explore::testing::{CheckSettings, TestDeviceRequest, assert_set_conforms, headless_test_device};
use henad_graph::GraphFile;
use henad_graph::document::World;
use henad_graph::sheet::starters;

use crate::gpu_game_of_life::words_per_row;

/// Returns a headless device with the limits of `wgpu::Limits::default()`, or `None` to skip a GPU test on a machine
/// without a device.
///
/// Every example GPU model has to build on this device, the WebGPU baseline.
///
/// Panics when `HENAD_REQUIRE_GPU` is set and no device is available.
pub(super) fn baseline_device() -> Option<GpuContext> {
    headless_test_device(&TestDeviceRequest::baseline())
}

/// Asserts that the entries of `entries`, each with its name, pass every check of the testing kit, their GPU checks on
/// a baseline device included when this machine has one.
///
/// Panics if the model set refuses an entry, such as one whose id another holds.
pub(super) fn assert_entries_conform(entries: impl IntoIterator<Item = (String, ModelEntry)>) {
    let mut models = ModelSet::new(henad_core::build_info!());
    for (name, entry) in entries {
        models
            .insert(entry)
            .unwrap_or_else(|error| panic!("{name}: the set refused the entry: {error}"));
    }
    let mut settings = CheckSettings::default();
    if let Some(device) = baseline_device() {
        settings = settings.gpu(device);
    }
    assert_set_conforms(&models, &settings);
}

/// Returns the entry the model file `text` builds.
///
/// Panics with the file's diagnostics if the text does not read or the entry does not build, the model id first when
/// the text reads.
pub(super) fn model_entry(text: &str) -> ModelEntry {
    let file = ModelFile::parse(text).unwrap_or_else(|error| panic!("{error}"));
    file.entry().unwrap_or_else(|error| panic!("{}: {error}", file.id()))
}

/// Returns the text of the graph starter `file_name`.
pub(super) fn starter_text(file_name: &str) -> &'static str {
    henad_graph::starters::starter(file_name)
        .unwrap_or_else(|| panic!("no starter {file_name}"))
        .text
}

/// Returns the entry the model file of the graph starter `file_name` builds.
pub(super) fn starter_entry(file_name: &str) -> ModelEntry {
    model_entry(starter_text(file_name))
}

/// Returns the text of `tests/fixtures/<path>`.
pub(super) fn fixture_text(path: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// Returns the name and text of every file of `tests/fixtures/<directory>` whose extension is `extension`, sorted by
/// name.
pub(super) fn fixture_files(directory: &str, extension: &str) -> Vec<(String, String)> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(directory);
    let mut files: Vec<(String, String)> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|found| found == extension))
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let name = path.file_name().expect("a file name").to_string_lossy().into_owned();
            (name, text)
        })
        .collect();
    files.sort();
    files
}

/// Returns every graph of the corpus in a world `world` accepts, with its file name: henad-graph's starters first,
/// then the fixtures of `tests/fixtures/graphs`, each read through a [`ModelFile`] as a graph.
pub(super) fn graph_corpus(world: fn(&World) -> bool) -> Vec<(String, GraphFile)> {
    let starters = henad_graph::starters::starters()
        .iter()
        .chain(henad_graph::starters::agent_starters())
        .map(|starter| (starter.file_name.to_owned(), starter.text.to_owned()));
    starters
        .chain(fixture_files("graphs", "toml"))
        .filter_map(|(name, text)| {
            let file = ModelFile::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
            let graph = file
                .graph()
                .unwrap_or_else(|| panic!("{name} reads as a graph"))
                .clone();
            world(&graph.document().world).then_some((name, graph))
        })
        .collect()
}

/// Returns every agents graph of the corpus with its file name, the agent starters first.
pub(super) fn agent_corpus() -> Vec<(String, GraphFile)> {
    graph_corpus(|world| matches!(world, World::Agents(_)))
}

/// Returns the agent sheet starters with their file names.
pub(super) fn agent_sheets() -> Vec<(&'static str, &'static str)> {
    starters()
        .iter()
        .filter(|starter| starter.text.contains("made_of = \"agents\""))
        .map(|starter| (starter.file_name, starter.text))
        .collect()
}

/// Returns the declarations of `plain` for an entry built from another spec of the same model, whose display layers
/// are `topology_hint` and whose storage layout is `structure`.
///
/// Every other declaration is the one `plain` holds: id, name, description, parameters, stats, actions, palette,
/// replay and time scale.
pub(super) fn declarations_like(
    plain: &ModelEntry,
    topology_hint: TopologyHint,
    structure: Structure,
) -> EntryDeclarations {
    let metadata = plain.metadata();
    let mut declarations = EntryDeclarations::new(
        plain.id().to_owned(),
        plain.name().to_owned(),
        plain.description().to_owned(),
        plain.param_descriptors().to_vec(),
        topology_hint,
        structure,
    );
    declarations.stats = plain.stat_descriptors().to_vec();
    declarations.actions = plain.action_descriptors().to_vec();
    declarations.palette.clone_from(&metadata.palette);
    declarations.replays_exactly = metadata.replays_exactly;
    declarations.time.clone_from(&metadata.time);
    declarations
}

/// Returns the parameters of `entry` at their defaults, with each id in `overrides` set to its value.
///
/// Panics if an id of `overrides` names no parameter of `entry`.
pub(super) fn params_for(entry: &ModelEntry, overrides: &[(&str, ParamValue)]) -> Vec<ParamValue> {
    for (id, _) in overrides {
        assert!(
            entry
                .param_descriptors()
                .iter()
                .any(|descriptor| descriptor.id() == *id),
            "{}: no parameter {id}",
            entry.id()
        );
    }
    entry
        .param_descriptors()
        .iter()
        .map(|descriptor| {
            overrides
                .iter()
                .find(|(id, _)| *id == descriptor.id())
                .map_or_else(|| descriptor.kind().default_value(), |(_, value)| value.clone())
        })
        .collect()
}

/// Returns the build parameters of the grid model `entry` at `side` by `side` cells: the grid size, then each
/// parameter at its default unless `overrides` names it.
///
/// Panics if an id of `overrides` names no parameter of `entry`.
pub(super) fn device_params(entry: &ModelEntry, side: u32, overrides: &[(&str, f32)]) -> Vec<ParamValue> {
    let overrides: Vec<(&str, ParamValue)> = overrides
        .iter()
        .map(|&(id, value)| (id, ParamValue::F32(value)))
        .collect();
    let mut params = params_for(entry, &overrides);
    params[0] = ParamValue::U32(side);
    params[1] = ParamValue::U32(side);
    params
}

/// Builds `entry` on `ctx` as a GPU state.
pub(super) fn build_gpu(
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

/// Asserts that `ctx` builds `entry` at its defaults, steps it twice and reads its stats, as a device that survived a
/// fault runs another model.
pub(super) fn assert_device_runs(ctx: &GpuContext, entry: &ModelEntry) {
    let mut state = build_gpu(entry, &params_for(entry, &[]), None, ctx);
    step(ctx, &mut *state, 2);
    sample_stats(&mut *state, ctx).expect("the device runs another model after the fault");
}

/// Steps `state` `count` times in one submission.
pub(super) fn step(ctx: &GpuContext, state: &mut dyn GpuSimState, count: u32) {
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    state.encode_steps(&mut encoder, count, None);
    ctx.queue.submit(Some(encoder.finish()));
}

/// Presses action `index` of `state` in a submission of its own.
pub(super) fn press(ctx: &GpuContext, state: &mut dyn GpuSimState, index: usize) {
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    assert!(state.encode_action(&mut encoder, index), "action {index}");
    ctx.queue.submit(Some(encoder.finish()));
}

/// Runs the display and reduce passes of `state` and waits for its stats.
pub(super) fn snapshot(ctx: &GpuContext, state: &mut dyn GpuSimState) {
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    state.encode_snapshot_passes(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));
    state.begin_stats_readback();
    state.poll_stats_readback(&ctx.device, true);
}

/// Returns the scalar stat labelled `label`.
pub(super) fn stat(stats: &[StatEntry], label: &str) -> f64 {
    match stats.iter().find(|entry| entry.label() == label).map(StatEntry::value) {
        Some(StatValue::Scalar(value)) => *value,
        other => panic!("no scalar stat `{label}`: {other:?}"),
    }
}

/// Returns the stat values of `state` after a snapshot as plain numbers in declaration order, a vector as its two
/// components.
pub(super) fn stat_values(ctx: &GpuContext, state: &mut dyn GpuSimState) -> Vec<f64> {
    snapshot(ctx, state);
    state
        .stats()
        .iter()
        .flat_map(|stat| match stat.value() {
            StatValue::Scalar(value) => vec![*value],
            StatValue::Vector2D { x, y } => vec![*x, *y],
            StatValue::Histogram { .. } => panic!("{} reports a histogram", stat.label()),
        })
        .collect()
}

/// Returns the cells of the bit-packed rows `words` of `gpu_game_of_life` at `width` by `height` cells, one word per
/// cell in row-major order.
pub(super) fn unpacked_life_cells(words: &[u32], width: u32, height: u32) -> Vec<u32> {
    let stride = words_per_row(width);
    (0..height as usize)
        .flat_map(|y| (0..width as usize).map(move |x| (y, x)))
        .map(|(y, x)| (words[y * stride + x / 32] >> (x % 32)) & 1)
        .collect()
}

/// Returns the words of buffer `index` of `state`.
pub(super) fn buffer_words(state: &dyn GpuSimState, index: usize) -> Vec<u32> {
    state
        .read_buffer(index)
        .unwrap_or_else(|| panic!("buffer {index} reads back"))
}

/// Returns the position of the buffer labelled `label` among the buffers of the GPU agent model `entry`.
pub(super) fn buffer_index(entry: &ModelEntry, label: &str) -> usize {
    let Structure::GpuAgents { buffers, .. } = &entry.metadata().structure else {
        panic!("{} is no GPU agent model", entry.id());
    };
    buffers
        .iter()
        .position(|buffer| buffer.label == label)
        .unwrap_or_else(|| panic!("{} has no buffer {label}", entry.id()))
}

/// Returns the words of the buffer labelled `label` of `state`, built from the GPU agent model `entry`.
pub(super) fn graph_buffer(entry: &ModelEntry, state: &dyn GpuSimState, label: &str) -> Vec<u32> {
    state
        .read_buffer(buffer_index(entry, label))
        .unwrap_or_else(|| panic!("{label} reads back"))
}

/// Returns the words of lane `id` among `lanes`, each a lane id with its words.
pub(super) fn lane<'a>(lanes: &'a [(String, Vec<u32>)], id: &str) -> &'a [u32] {
    lanes
        .iter()
        .find(|(lane, _)| lane == id)
        .map(|(_, words)| words.as_slice())
        .unwrap_or_else(|| panic!("no lane {id}"))
}

/// Returns the components of the interleaved `vec2<f32>` lane `words`.
pub(super) fn pairs(words: &[u32]) -> Vec<(f32, f32)> {
    words
        .chunks_exact(2)
        .map(|pair| (f32::from_bits(pair[0]), f32::from_bits(pair[1])))
        .collect()
}

/// Returns the limits of a device that holds a buffer and a binding of any size, the WebGPU defaults otherwise.
pub(super) fn roomy_limits() -> wgpu::Limits {
    wgpu::Limits {
        max_storage_buffer_binding_size: u64::MAX,
        max_buffer_size: u64::MAX,
        ..wgpu::Limits::default()
    }
}
