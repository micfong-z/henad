//! Lane packing: a model past the fixed budget of 8 storage bindings compiles to shaders within it, the same shaders
//! whatever device it later builds on, and a packed model steps as its unpacked twin does.

use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::fault::FaultSink;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::gpu::stepping::run_steps;
use henad_compute::gpu::{GpuContext, wgpu};
use henad_core::metadata::Structure;
use henad_core::params::ParamValue;

use super::support::{TestDevice, baseline_test_device, test_device};
use crate::document::World;
use crate::limits::MAX_STORAGE_BINDINGS;
use crate::sheet::{SheetFile, starters};
use crate::wgsl::pack::{UNPACKED, storage_bindings};
use crate::wgsl::{AgentProgram, GeneratedShader, GridProgram, LaneBuffer};

/// Text of the test sheet of three infections, each read at the neighbours of every cell.
const THREE_STRAINS: &str = include_str!("sheets/three_strains.toml");

/// Returns the text of the sheet starter `file_name`.
fn sheet(file_name: &str) -> &'static str {
    starters()
        .iter()
        .find(|starter| starter.file_name == file_name)
        .map(|starter| starter.text)
        .unwrap_or_else(|| panic!("no sheet starter {file_name}"))
}

/// Returns `text` with `old` replaced by `new` once, where `text` must hold `old`.
fn edited(text: &str, old: &str, new: &str) -> String {
    assert!(text.contains(old), "the text holds no {old:?}");
    text.replacen(old, new, 1)
}

/// Returns the property table of a Number property `id` that starts at 0, for the kind of `table`.
fn number_property(table: &str, id: &str) -> String {
    format!(
        "[[{table}]]\nid = \"{id}\"\nname = \"Energy\"\ntype = \"number\"\n\n[{table}.starts_as]\nrule = \"value\"\nvalue = 0\n\n"
    )
}

/// Returns the line `line` that sets the property `id` to `formula` each time it runs.
fn formula_line(line: &str, id: &str, formula: &str) -> String {
    format!(
        "[card.line.{line}]\nkind = \"set\"\nproperty = \"{id}\"\n\n[card.line.{line}.value]\nkind = \"calculate\"\n\
         formula = \"{formula}\"\n\n[card.line.{line}.condition]\nkind = \"always\"\n\n"
    )
}

/// Returns the line `line` that adds 1 to the property `id` each time it runs.
fn add_one_line(line: &str, id: &str) -> String {
    formula_line(line, id, &format!("my.{id} + 1"))
}

/// Returns the ants starter with one more Number property and a line that adds 1 to it each tick.
fn ants_with_energy() -> String {
    let text = edited(sheet("ants.toml"), "next_line = 17", "next_line = 18");
    let text = edited(&text, "\"l12\", \"l16\"]", "\"l12\", \"l16\", \"l17\"]");
    let text = edited(
        &text,
        "[ground]\n",
        &format!("{}[ground]\n", number_property("agents.property", "energy")),
    );
    format!("{text}\n{}", add_one_line("l17", "energy"))
}

/// Returns the ants starter with three more properties after its own, each set by a line every tick: a Number that
/// gains 1, a Whole number that starts at -5 and loses 2, and an Arrow that starts in a random direction and doubles.
///
/// The three pack after the Cargo and Reward held lanes. The Whole number is an `i32` lane at a non-zero offset, and
/// the Arrow a `vec2<f32>` lane at a non-zero offset.
fn ants_with_three_kinds() -> String {
    let text = edited(sheet("ants.toml"), "next_line = 17", "next_line = 20");
    let text = edited(
        &text,
        "\"l12\", \"l16\"]",
        "\"l12\", \"l16\", \"l17\", \"l18\", \"l19\"]",
    );
    let properties = format!(
        "{}[[agents.property]]\nid = \"tally\"\nname = \"Tally\"\ntype = \"whole_number\"\n\n\
         [agents.property.starts_as]\nrule = \"value\"\nvalue = -5\n\n\
         [[agents.property]]\nid = \"drift\"\nname = \"Drift\"\ntype = \"arrow\"\n\n\
         [agents.property.starts_as]\nrule = \"value\"\nkey = 77\n\n\
         [agents.property.starts_as.value]\nkind = \"random_direction\"\nlength = 1\n\n",
        number_property("agents.property", "energy")
    );
    let text = edited(&text, "[ground]\n", &format!("{properties}[ground]\n"));
    format!(
        "{text}\n{}{}{}",
        add_one_line("l17", "energy"),
        formula_line("l18", "tally", "my.tally - 2"),
        formula_line("l19", "drift", "my.drift * 2")
    )
}

/// Returns the boids starter with one more Number property and a line that adds 1 to it each tick.
fn boids_with_energy() -> String {
    let text = edited(sheet("boids.toml"), "next_line = 9", "next_line = 10");
    let text = edited(&text, "\"l6\", \"l7\"]", "\"l6\", \"l7\", \"l9\"]");
    let text = edited(
        &text,
        "[[parameter]]\n",
        &format!("{}[[parameter]]\n", number_property("agents.property", "energy")),
    );
    edited(
        &text,
        "# Lines of Randomise headings",
        &format!("{}# Lines of Randomise headings", add_one_line("l9", "energy")),
    )
}

/// Returns the three strains with one more Number property and a line that adds 1 to it each tick.
fn three_strains_with_age() -> String {
    let text = edited(THREE_STRAINS, "next_line = 10", "next_line = 11");
    let text = edited(&text, "\"l8\", \"l9\"]", "\"l8\", \"l9\", \"l10\"]");
    let text = edited(
        &text,
        "[[parameter]]\n",
        &format!("{}[[parameter]]\n", number_property("cells.property", "age")),
    );
    format!("{text}\n{}", add_one_line("l10", "age"))
}

/// A graph compiled for the GPU, on a grid or of agents.
enum Program {
    Grid(GridProgram),
    Agents(AgentProgram),
}

impl Program {
    /// Lowers the sheet `text` and compiles it, with every lane in a buffer of its own when `unpacked` holds.
    fn of(text: &str, unpacked: bool) -> Self {
        let sheet = SheetFile::parse(text).unwrap_or_else(|error| panic!("the sheet does not lower: {error}"));
        UNPACKED.with(|cell| cell.set(unpacked));
        let file = sheet.lowered();
        let program = if matches!(file.document().world, World::Grid(_)) {
            Self::Grid(GridProgram::compile(file).unwrap_or_else(|problems| panic!("{problems:#?}")))
        } else {
            Self::Agents(
                AgentProgram::compile_with_fault_names(file, sheet.fault_names())
                    .unwrap_or_else(|problems| panic!("{problems:#?}")),
            )
        };
        UNPACKED.with(|cell| cell.set(false));
        program
    }

    fn shaders(&self) -> &[GeneratedShader] {
        match self {
            Self::Grid(program) => program.shaders(),
            Self::Agents(program) => program.shaders(),
        }
    }

    fn entry(&self) -> ModelEntry {
        match self {
            Self::Grid(program) => program.entry(),
            Self::Agents(program) => program.entry(),
        }
    }

    fn lane_buffer(&self, id: &str) -> Option<LaneBuffer> {
        match self {
            Self::Grid(program) => program.lane_buffer(id),
            Self::Agents(program) => program.lane_buffer(id),
        }
    }

    /// Largest number of storage buffers one shader binds.
    fn widest(&self) -> usize {
        self.shaders()
            .iter()
            .map(|shader| storage_bindings(shader.bindings()))
            .max()
            .unwrap_or_default()
    }
}

/// Returns a context over `device`.
fn context(device: TestDevice) -> GpuContext {
    GpuContext::new(
        device.device,
        device.queue,
        wgpu::TextureFormat::Rgba8Unorm,
        FaultSink::new(),
    )
}

/// Returns the parameters of `entry` at their defaults, with `agents` agents in a world of agents.
fn params_of(entry: &ModelEntry, agents: u32) -> Vec<ParamValue> {
    entry
        .param_descriptors()
        .iter()
        .map(|descriptor| match descriptor.id() {
            "num_agents" => ParamValue::U32(agents),
            "grid_width" | "grid_height" => ParamValue::U32(96),
            _ => descriptor.kind().default_value(),
        })
        .collect()
}

/// Builds `entry` on `ctx` at seed 7 and runs `ticks` ticks.
fn built(entry: &ModelEntry, ctx: &GpuContext, ticks: u64) -> Box<dyn GpuSimState> {
    let params = params_of(entry, 3_000);
    let mut state = match entry.build(&params, Some(7), Some(ctx)) {
        Ok(ModelState::Gpu(state)) => state,
        Ok(ModelState::Cpu(_)) => panic!("{} built on the CPU", entry.id()),
        Err(fault) => panic!("{} did not build: {fault}", entry.id()),
    };
    run_steps(&mut *state, ctx, ticks).unwrap_or_else(|fault| panic!("{} did not run: {fault}", entry.id()));
    state
}

/// Presses the action at position `action` of `state` in a submission of its own.
fn press(ctx: &GpuContext, state: &mut dyn GpuSimState, action: usize) {
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    assert!(state.encode_action(&mut encoder, action), "action {action} runs");
    ctx.queue.submit(Some(encoder.finish()));
}

/// Returns the labels of the buffers of `entry`, a model of agents, and none for a grid.
fn buffer_labels(entry: &ModelEntry) -> Vec<String> {
    match &entry.metadata().structure {
        Structure::GpuAgents { buffers, .. } => buffers.iter().map(|buffer| buffer.label.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// The ants and boids starters with one more Number property, and the three strains with one more, bind more than 8
/// storage buffers in one shader unpacked, and compile to shaders within 8 that build and run on a device of the
/// WebGPU default limits. The three strains alone sit at the budget and keep every lane in a buffer of its own.
#[test]
#[expect(
    clippy::print_stderr,
    reason = "the skip is information for a reader of the test output"
)]
fn a_model_past_8_bindings_compiles_to_8() {
    let budget = MAX_STORAGE_BINDINGS as usize;
    let at_budget = Program::of(THREE_STRAINS, false);
    assert_eq!(at_budget.widest(), budget, "the three strains bind 8 storage buffers");
    assert!(
        ["health_a", "health_b", "health_c"]
            .iter()
            .all(|id| at_budget.lane_buffer(id).is_some_and(|place| !place.is_packed())),
        "a model within the budget packs nothing"
    );
    let models = [
        ("ants", ants_with_energy(), "energy"),
        ("boids", boids_with_energy(), "energy"),
        ("three strains", three_strains_with_age(), "health_a"),
    ];
    let device = baseline_test_device();
    if device.is_none() {
        eprintln!("skipping the device half of a_model_past_8_bindings_compiles_to_8: no adapter");
    }
    let ctx = device.map(context);
    for (name, text, packed_lane) in &models {
        assert!(
            Program::of(text, true).widest() > budget,
            "{name} binds past 8 unpacked"
        );
        let program = Program::of(text, false);
        assert!(
            program.widest() <= budget,
            "{name} binds {} storage buffers",
            program.widest()
        );
        assert!(
            program.lane_buffer(packed_lane).is_some_and(|place| place.is_packed()),
            "{name} packs {packed_lane}"
        );
        if let Some(ctx) = &ctx {
            built(&program.entry(), ctx, 3);
        }
    }
}

/// A packed file compiles to the same shaders each time, since the compiler reads no device, and the same entry builds
/// on a device of the WebGPU default limits and on one of raised limits.
#[test]
#[expect(
    clippy::print_stderr,
    reason = "the skip is information for a reader of the test output"
)]
fn packing_never_depends_on_the_device() {
    let text = ants_with_energy();
    let first = Program::of(&text, false);
    let second = Program::of(&text, false);
    let texts = |program: &Program| -> Vec<String> {
        program
            .shaders()
            .iter()
            .map(|shader| shader.composed().to_owned())
            .collect()
    };
    assert_eq!(texts(&first), texts(&second));
    assert!(
        buffer_labels(&first.entry())
            .iter()
            .any(|label| label == "lanes_agents_0")
    );
    let (Some(baseline), Some(raised)) = (baseline_test_device(), test_device()) else {
        eprintln!("skipping the device half of packing_never_depends_on_the_device: no adapter");
        return;
    };
    let entry = first.entry();
    for ctx in [context(baseline), context(raised)] {
        let state = built(&entry, &ctx, 2);
        let place = first.lane_buffer("energy").expect("the energy lane");
        let words = state.read_buffer(place.buffer()).expect("the packed buffer reads back");
        assert!(
            place.segment(&words).iter().all(|&word| f32::from_bits(word) == 2.0),
            "two ticks add 2 to every ant's energy"
        );
    }
}

/// The ants starter with three more properties and the three strains with one more Number property step as their
/// unpacked twins do on a device that allows both: every lane equal after 20 ticks from one seed, and for the ants
/// after a press of Reset colony and 5 more ticks, every other buffer too.
///
/// The packed ants hold a `u32`, two `f32`, an `i32` and a `vec2<f32>` lane in one buffer, the last two at a non-zero
/// offset, and the three strains hold `u32` lanes on two sides.
#[test]
#[expect(
    clippy::print_stderr,
    reason = "the skip is information for a reader of the test output"
)]
fn a_packed_model_steps_as_its_unpacked_twin() {
    let Some(device) = test_device() else {
        eprintln!("skipping a_packed_model_steps_as_its_unpacked_twin: no adapter");
        return;
    };
    let ctx = context(device);
    for text in [ants_with_three_kinds(), three_strains_with_age()] {
        let sheet = SheetFile::parse(&text).unwrap_or_else(|error| panic!("{error}"));
        let packed = Program::of(&text, false);
        let unpacked = Program::of(&text, true);
        if let Program::Agents(program) = &packed {
            assert!(program.replays_exactly(), "the ants replay at one seed");
            for id in ["tally", "drift"] {
                let place = program.lane_buffer(id).expect("a lane on the device");
                assert!(place.is_packed() && place.offset() > 0, "{id} packs after another lane");
            }
        }
        let (packed_entry, unpacked_entry) = (packed.entry(), unpacked.entry());
        let mut packed_state = built(&packed_entry, &ctx, 20);
        let mut unpacked_state = built(&unpacked_entry, &ctx, 20);
        // Reset colony starts the ants again, and its rerun uploads the packed buffer whole.
        if !packed_entry.action_descriptors().is_empty() {
            for state in [&mut packed_state, &mut unpacked_state] {
                press(&ctx, &mut **state, 0);
                run_steps(&mut **state, &ctx, 5).unwrap_or_else(|fault| panic!("{fault}"));
            }
        }
        let mut seen_packed = false;
        for lane in &sheet.lowered().document().lanes {
            let read = |program: &Program, state: &dyn GpuSimState| {
                let place = program.lane_buffer(&lane.id).expect("a lane on the device");
                let words = state.read_buffer(place.buffer()).expect("the lane's buffer reads back");
                (place.is_packed(), place.segment(&words).to_vec())
            };
            let (is_packed, found) = read(&packed, &*packed_state);
            let (was_packed, expected) = read(&unpacked, &*unpacked_state);
            assert!(!was_packed, "the twin packs nothing");
            seen_packed |= is_packed;
            assert!(
                found == expected,
                "{}: the lane {} differs from its twin",
                packed_entry.id(),
                lane.id
            );
        }
        assert!(seen_packed, "{} packs a lane", packed_entry.id());
        let labels = buffer_labels(&unpacked_entry);
        for (index, label) in buffer_labels(&packed_entry).iter().enumerate() {
            if label.starts_with("lane") {
                continue;
            }
            let twin = labels
                .iter()
                .position(|known| known == label)
                .expect("the twin has the buffer");
            assert!(
                packed_state.read_buffer(index) == unpacked_state.read_buffer(twin),
                "{}: the buffer {label} differs from its twin",
                packed_entry.id()
            );
        }
    }
}
