//! Tests of [`crate::gpu::runtime_grid`]: the checks, the places of their diagnostics, the template's layout and
//! bindings, and a rule stepped on a device.

use henad_core::authoring::model::binding::BindingDecl;
use henad_core::authoring::primitives::rng::pcg_hash;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::helpers::{bool_param, choice_param, f32_param, u32_param};
use henad_core::params::{ChoiceOptions, ParamDescriptor, ParamKind, ParamValue};
use henad_core::provenance::ModelCode;
use henad_core::view::StatValue;

use super::support::headless_context;
use crate::entry::ModelState;
use crate::gpu::compose::split_span;
use crate::gpu::grid_spec::GridHooks as _;
use crate::gpu::runtime_grid::test_support::{
    DISPLAY_BINDINGS, MAX_FUNCTION_BYTES, MAX_PRIVATE_BYTES, MAX_STEP_COST, REDUCE_BINDINGS, STEP_BINDINGS, composed,
    costs,
};
use crate::gpu::runtime_grid::{GridRule, MAX_STEP_BYTES, RuleDeclaration, RuleState};

/// Returns the Life rule with `step` as its text.
fn life_with(step: &str) -> GridRule {
    let mut rule = GridRule::life();
    rule.step = step.to_owned();
    rule
}

/// Returns the problems `rule` compiles to.
///
/// # Panics
///
/// Panics if the rule compiles.
fn refusals(rule: &GridRule) -> Vec<CodeDiagnostic> {
    match rule.compile() {
        Ok(_) => panic!("the rule compiled:\n{}", rule.step),
        Err(problems) => problems,
    }
}

/// Returns the line and column of each problem.
fn places(problems: &[CodeDiagnostic]) -> Vec<(Option<u32>, Option<u32>)> {
    problems.iter().map(|problem| (problem.line, problem.column)).collect()
}

/// Asserts that one problem of `problems` sits at `line`, and that its message holds `text`.
fn assert_problem_at(problems: &[CodeDiagnostic], line: u32, text: &str) {
    assert!(
        problems
            .iter()
            .any(|problem| problem.line == Some(line) && problem.message.contains(text)),
        "no problem at line {line} saying `{text}`: {problems:#?}"
    );
}

const STEP_HEAD: &str = "fn step_cell(cell: u32, rng: ptr<function, u32>) -> u32 {\n";

/// `naga_oil` puts a module's index above bit 21 of each span, and the entry's text behind a header. The masked index of
/// an imported function names its module, and the place of each item of the rule's text is its place as written.
#[test]
fn naga_oil_span_encoding_is_unchanged() {
    // Imported names on the lines in front of `step_cell` lengthen those lines in the text naga parses.
    let step = "#import henad::rng::{next_bits, pcg_hash}\n#import henad::space::{wrap_index}\n\
                fn helper(x: u32) -> u32 { return pcg_hash(x) ^ u32(wrap_index(i32(x), 7)); }\n\
                const ÉCHELLE: u32 = 3u;\n\
                fn step_cell(cell: u32, rng: ptr<function, u32>) -> u32 {\n    \
                return helper(next_bits(rng)) % ÉCHELLE;\n}\n";
    let rule = life_with(step);
    let (composition, start) = composed(&rule).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let module = &composition.module;

    // The shared modules take indices 1 to 5 in `SHARED_WGSL_MODULES` order: dispatch, dims, rng, space, reduce_tree.
    let index_of = |name: &str| {
        let (handle, _) = module
            .functions
            .iter()
            .find(|(_, function)| function.name.as_deref().is_some_and(|text| text.starts_with(name)))
            .unwrap_or_else(|| panic!("no function `{name}` in the module"));
        split_span(module.functions.get_span(handle)).map(|(index, _)| index)
    };
    assert_eq!(index_of("pcg_hashX_naga_oil_mod_X"), Some(3), "henad::rng");
    assert_eq!(index_of("wrap_indexX_naga_oil_mod_X"), Some(4), "henad::space");
    assert_eq!(index_of("step_cell"), Some(0), "the entry");

    for (name, written) in [("helper", "fn helper"), ("step_cell", "fn step_cell")] {
        let (handle, _) = module
            .functions
            .iter()
            .find(|(_, function)| function.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no function `{name}`"));
        let located = composition
            .locate_in_entry(module.functions.get_span(handle), start)
            .unwrap_or_else(|| panic!("`{name}` has no place in the rule's text"));
        assert_eq!(located.span.start, step.find(written).unwrap_or_default(), "{name}");
    }
    assert!(life_with(step).compile().is_ok());
}

/// A parse error points at its line and character column in the rule's text, after imported names and multi-byte
/// characters on the same line and on lines before it.
#[test]
fn a_syntax_error_points_at_the_user_line_and_column() {
    let step = "// Règle « vivante » à l'état brut\n#import henad::rng::{next_float}\n\
                fn step_cell(cell: u32, rng: ptr<function, u32>) -> u32 {\n    \
                let tirage = next_float(rng, 1.0); let é = 2u; let x = é + ;\n    \
                return cell;\n}\n";
    let problems = refusals(&life_with(step));
    assert_eq!(problems.len(), 1, "{problems:#?}");
    let problem = &problems[0];
    let line = "    let tirage = next_float(rng, 1.0); let é = 2u; let x = é + ;";
    let semicolon = line.rfind(';').unwrap_or_default();
    let column = line[..semicolon].chars().count() as u32 + 1;
    assert_eq!((problem.line, problem.column), (Some(4), Some(column)), "{problem}");
    let line_start = step.find(line).unwrap_or_default();
    assert_eq!(
        problem.span.as_ref().map(|span| span.start),
        Some(line_start + semicolon)
    );
    assert_eq!(problem.file, None);

    // A validation error is placed at the expression at fault, not at the start of its function.
    let step = format!("{STEP_HEAD}    let a = 1u;\n    let wide = i64(cell);\n    return cell;\n}}\n");
    let problems = refusals(&life_with(&step));
    assert_eq!(places(&problems), [(Some(3), Some(16))], "{problems:#?}");
    assert!(
        !problems[0].notes.iter().any(|note| note.contains("the engine calls")),
        "{problems:#?}"
    );
}

/// A `step_cell` the template cannot call breaks the template, and the diagnostic has no place in the rule's text.
#[test]
fn an_error_inside_the_template_has_no_user_line() {
    let problems = refusals(&life_with("fn step_cell(cell: u32) -> u32 {\n    return cell;\n}\n"));
    assert_eq!(places(&problems), [(None, None)], "{problems:#?}");
    assert!(problems[0].span.is_none());
    assert!(
        problems[0].notes.iter().any(|note| note.contains("ptr<function, u32>")),
        "{problems:#?}"
    );
}

#[test]
fn a_loop_in_user_code_is_refused_at_its_line() {
    let step = "fn spin() {\n    loop {\n        break;\n    }\n}\n\
                fn step_cell(cell: u32, rng: ptr<function, u32>) -> u32 {\n    \
                var total = 0u;\n    \
                for (var i = 0u; i < 3u; i++) {\n        total += i;\n    }\n    \
                while (total > 9u) {\n        total -= 1u;\n    }\n    \
                spin();\n    return total;\n}\n";
    let problems = refusals(&life_with(step));
    assert_eq!(
        places(&problems),
        [(Some(2), Some(5)), (Some(8), Some(5)), (Some(11), Some(5))],
        "{problems:#?}"
    );
    assert!(
        problems
            .iter()
            .all(|problem| problem.message.starts_with("loops are not allowed"))
    );
}

#[test]
fn bindings_and_foreign_directives_in_user_text_are_refused() {
    let step = format!(
        "@group(0) @binding(5) var<storage, read_write> extra: array<u32>;\n#ifdef FAST\n#define_import_path mine\n\
         enable f16;\nfn neighbor(dx: i32, dy: i32) -> u32 {{ return 0u; }}\n{STEP_HEAD}    \
         return henad::rng::pcg_hash(cell);\n}}\n"
    );
    let problems = refusals(&life_with(&step));
    assert_problem_at(&problems, 1, "declares no bindings");
    assert_problem_at(&problems, 5, "`neighbor` is a name of the rule template");
    assert_eq!(
        problems.iter().filter(|problem| problem.line == Some(1)).count(),
        1,
        "one problem per binding: {problems:#?}"
    );

    // The directive check runs once the names pass.
    let step = format!(
        "#ifdef FAST\n#define_import_path mine\nenable f16;\n{STEP_HEAD}    return henad::rng::pcg_hash(cell);\n}}\n"
    );
    let problems = refusals(&life_with(&step));
    assert_problem_at(&problems, 1, "only `#import` lines");
    assert_problem_at(&problems, 2, "declares no `#define_import_path`");
    assert_problem_at(&problems, 3, "`enable` directive");
    assert_problem_at(&problems, 5, "no `::`");

    // A binding the text check cannot see, behind a comment break, is refused in the module.
    let step = format!(
        "@group(0)\n/* */ @binding(5)\nvar<storage, read_write> extra: array<u32>;\n{STEP_HEAD}    return cell;\n}}\n"
    );
    assert!(!refusals(&life_with(&step)).is_empty());

    // An entry point of the rule's own has no use, however its attribute is written.
    let step = format!("@compute @workgroup_size(1)\nfn other() {{}}\n{STEP_HEAD}    return cell;\n}}\n");
    assert_problem_at(&refusals(&life_with(&step)), 1, "declares no entry point");
    let step = format!(
        "@ /* spaced */ compute @workgroup_size(1)\nfn other() {{ loop {{}} }}\n{STEP_HEAD}    return cell;\n}}\n"
    );
    assert_problem_at(&refusals(&life_with(&step)), 1, "declares no entry point");

    // A module-scope item cannot take a built-in's name, which the template's own lines call. A local can.
    let step = format!(
        "fn min(a: u32, b: u32) -> u32 {{ return 7u; }}\nalias vec2 = vec3<f32>;\n{STEP_HEAD}    let max = 1u;\n    \
         return cell + max;\n}}\n"
    );
    let problems = refusals(&life_with(&step));
    assert_problem_at(&problems, 1, "`min` is a predeclared name");
    assert_problem_at(&problems, 2, "`vec2` is a predeclared name");
    assert_eq!(problems.len(), 2, "{problems:#?}");

    // An import from the shared modules is allowed.
    let step = format!("#import henad::rng::{{pcg_hash}}\n{STEP_HEAD}    return pcg_hash(cell) % NUM_STATES;\n}}\n");
    assert!(life_with(&step).compile().is_ok());
}

#[test]
fn shared_writes_and_barriers_are_refused() {
    let step = format!(
        "var<private> mine: u32;\nvar<workgroup> shared_total: u32;\n{STEP_HEAD}    \
         mine = cell;\n    state_out[0] = 1u;\n    tick_out[0] = 0u;\n    henad_cell = vec2<u32>(0u, 0u);\n    \
         workgroupBarrier();\n    return mine;\n}}\n"
    );
    let problems = refusals(&life_with(&step));
    assert_problem_at(&problems, 2, "outside `private`");
    assert_problem_at(&problems, 5, "`state_out` is a buffer of the engine");
    assert_problem_at(&problems, 6, "`tick_out` is a buffer of the engine");
    assert_problem_at(&problems, 7, "writes `henad_cell`");
    assert_problem_at(&problems, 8, "barriers are not allowed");
    assert!(
        !problems.iter().any(|problem| problem.line == Some(4)),
        "a write to the rule's own private variable is allowed: {problems:#?}"
    );
    assert_eq!(
        problems.iter().filter(|problem| problem.line == Some(5)).count(),
        1,
        "one problem per write to an engine buffer: {problems:#?}"
    );

    // Reading a buffer that other cells write in the same pass, or the cells by index, is refused as well.
    let step = format!(
        "fn far(cell: u32) -> u32 {{\n    return state_in[cell * 99999999u];\n}}\n{STEP_HEAD}    \
         return state_out[0] + tick_out[0] + tick_in[0] + far(cell) + params.width;\n}}\n"
    );
    let problems = refusals(&life_with(&step));
    assert_problem_at(&problems, 2, "`state_in` is a buffer of the engine");
    for buffer in ["state_out", "tick_out", "tick_in"] {
        assert_problem_at(&problems, 5, &format!("`{buffer}` is a buffer of the engine"));
    }
    assert_eq!(problems.len(), 4, "`params` is read freely: {problems:#?}");

    let step = format!(
        "#import henad::reduce_tree::{{block_sum}}\nvar<workgroup> counter: atomic<u32>;\n{STEP_HEAD}    \
         let total = block_sum(0u, 1.0);\n    atomicAdd(&counter, 1u);\n    return cell;\n}}\n"
    );
    let problems = refusals(&life_with(&step));
    assert_problem_at(&problems, 2, "holds an atomic");
    assert_problem_at(&problems, 4, "this calls `block_sum`, which writes `scratch`");
    assert_problem_at(&problems, 5, "atomics are not allowed");

    // A pointer into storage, handed to a helper, would write the memory `state_out` writes.
    let step = format!(
        "fn poke(cell_out: ptr<storage, u32, read_write>) {{\n    *cell_out = 1u;\n}}\n{STEP_HEAD}    \
         poke(&state_out[0]);\n    return cell;\n}}\n"
    );
    let problems = refusals(&life_with(&step));
    assert!(
        problems.iter().any(|problem| problem.line.is_some()),
        "a write through a storage pointer is refused at a line of the rule: {problems:#?}"
    );
}

/// Variables keep to WGSL's limits: 8192 bytes for the locals of one function, and 8192 for every `private` variable
/// of the shader, the template's `henad_cell` included. The text keeps to 64 KiB.
#[test]
fn oversized_arrays_and_text_are_refused() {
    assert_eq!((MAX_FUNCTION_BYTES, MAX_PRIVATE_BYTES), (8192, 8192));
    let step = format!(
        "{STEP_HEAD}    var table: array<u32, 2048>;\n    return table[cell];\n}}\n\
         fn rule_of_nine(index: u32) -> u32 {{\n    var births: array<u32, 512>;\n    return births[index];\n}}\n"
    );
    assert!(
        life_with(&step).compile().is_ok(),
        "a function's locals can take 8192 bytes"
    );

    let step = format!(
        "var<private> big: array<u32, 2047>;\n\
         struct Wide {{\n    a: array<u32, 1024>,\n    b: array<u32, 1024>,\n    c: array<u32, 1024>,\n}}\n\
         fn wide() -> u32 {{\n    var w: Wide;\n    return w.a[0];\n}}\n\
         fn nested() -> f32 {{\n    var grid: array<array<f32, 64>, 64>;\n    return grid[0][0];\n}}\n\
         {STEP_HEAD}    var low: array<u32, 1024>;\n    var high: array<u32, 1025>;\n    \
         return big[cell] + low[0] + high[0] + wide() + u32(nested());\n}}\n"
    );
    let problems = refusals(&life_with(&step));
    assert_problem_at(&problems, 1, "`private` variables to 8196 bytes");
    assert_problem_at(&problems, 8, "local variables of `wide` to 12288 bytes");
    assert_problem_at(&problems, 12, "local variables of `nested` to 16384 bytes");
    assert_problem_at(&problems, 17, "local variables of `step_cell` to 8196 bytes");

    let padding = "//".repeat(MAX_STEP_BYTES / 2);
    let problems = refusals(&life_with(&format!("{padding}\n{}", GridRule::life().step)));
    assert_eq!(places(&problems), [(None, None)]);
    assert!(problems[0].message.contains("at most"), "{problems:#?}");
}

/// A step that calls a chain of helpers, each calling the one before twice, holds no loop and still runs for ever.
/// The call that takes a step past its work limit is refused.
#[test]
fn a_call_chain_past_the_work_limit_is_refused() {
    let mut step = String::from("fn g0(c: u32) -> u32 {\n    return c + 1u;\n}\n");
    for level in 1..=60 {
        step.push_str(&format!(
            "fn g{level}(c: u32) -> u32 {{\n    return g{previous}(c) + g{previous}(c ^ 1u);\n}}\n",
            previous = level - 1
        ));
    }
    step.push_str(&format!("{STEP_HEAD}    return g60(cell) % NUM_STATES;\n}}\n"));
    let problems = refusals(&life_with(&step));
    assert_eq!(problems.len(), 1, "{problems:#?}");
    // Each level doubles the work below it. The first level past the limit is refused at its second call.
    let (composition, _) = composed(&life_with(&step)).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let function_costs = costs(&composition.module);
    let cost_of = |name: &str| {
        composition
            .module
            .functions
            .iter()
            .find(|(_, function)| function.name.as_deref() == Some(name))
            .map(|(handle, _)| function_costs[handle.index()])
            .unwrap_or_else(|| panic!("no function `{name}`"))
    };
    let level = (1..=60)
        .find(|level| cost_of(&format!("g{level}")) > MAX_STEP_COST)
        .unwrap_or_else(|| panic!("no level passes the limit"));
    assert!(problems[0].message.contains(&format!("`g{level}`")), "{problems:#?}");
    assert_eq!(problems[0].line, Some(3 * level + 2), "{problems:#?}");

    // Life's step stays far below the limit.
    let (composition, _) = composed(&GridRule::life()).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let function_costs = costs(&composition.module);
    let step_cell = composition
        .module
        .functions
        .iter()
        .find(|(_, function)| function.name.as_deref() == Some("step_cell"))
        .map(|(handle, _)| function_costs[handle.index()]);
    assert!(step_cell.is_some_and(|cost| cost < MAX_STEP_COST / 32), "{step_cell:?}");
}

#[test]
fn a_param_named_seed_is_refused() {
    for id in ["seed", "width", "tick", "grid_width", "loop", "henad_rate", "Rate"] {
        let mut rule = GridRule::life();
        rule.params.push(f32_param("rate", "Rate", 0.5, 0.0, 1.0, None));
        rule.params[1] = henad_core::params::ParamDescriptor::new(id.to_owned(), "Odd", rule.params[1].kind().clone());
        let problems = refusals(&rule);
        assert!(
            problems
                .iter()
                .any(|problem| problem.message.contains(&format!("'{id}'"))),
            "{id}: {problems:#?}"
        );
    }
}

/// A parameter's declaration is held to what its kind can hold: a default within its bounds, a positive step, option
/// names that text can select, and a percentage only for a fraction. Each problem names its parameter.
#[test]
fn a_parameter_its_kind_cannot_hold_is_refused() {
    let declarations = [
        ParamDescriptor::new(
            "reversed",
            "Reversed",
            ParamKind::F32 {
                min: 1.0,
                max: 0.0,
                default: 0.5,
                step: None,
            },
        ),
        ParamDescriptor::new(
            "stepless",
            "Stepless",
            ParamKind::F32 {
                min: 0.0,
                max: 1.0,
                default: 0.5,
                step: Some(0.0),
            },
        ),
        ParamDescriptor::new(
            "radius",
            "Radius",
            ParamKind::U32 {
                min: 1,
                max: 5,
                default: 9,
            },
        ),
        ParamDescriptor::new(
            "mode",
            "Mode",
            ParamKind::Choice {
                options: ChoiceOptions::new(vec!["calm".to_owned(), "1".to_owned()]),
                default: 0,
            },
        ),
        ParamDescriptor::new(
            "shape",
            "Shape",
            ParamKind::Choice {
                options: ChoiceOptions::new(vec!["square".to_owned(), "square".to_owned()]),
                default: 0,
            },
        ),
        ParamDescriptor::new(
            "empty",
            "Empty",
            ParamKind::Choice {
                options: ChoiceOptions::new(Vec::new()),
                default: 0,
            },
        ),
        u32_param("count", "Count", 2, 1, 5).percent(),
    ];
    let mut rule = GridRule::life();
    rule.params.extend(declarations.iter().cloned());
    let problems = rule
        .check_declarations()
        .err()
        .unwrap_or_else(|| panic!("the declarations passed"));
    let expected = [
        (1, "outside its bounds"),
        (2, "step that is not a positive"),
        (3, "outside its bounds"),
        (4, "option '1' reads as an index"),
        (5, "option 'square' appears twice"),
        (6, "no option at its default index"),
        (7, "shows as a percentage"),
    ];
    for (index, text) in expected {
        assert!(
            problems
                .iter()
                .any(|(site, problem)| *site == Some(RuleDeclaration::Param(index)) && problem.message.contains(text)),
            "parameter {index} has no problem saying `{text}`: {problems:#?}"
        );
    }
    assert_eq!(problems.len(), expected.len(), "{problems:#?}");
    assert_eq!(refusals(&rule).len(), expected.len());
}

#[test]
fn two_states_with_one_constant_name_are_refused() {
    let mut rule = GridRule::life();
    rule.states.push(RuleState::new("dead!", [0, 0, 0, 255]));
    let problems = refusals(&rule);
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert!(
        problems[0].message.contains("both give the constant `DEAD`"),
        "{problems:#?}"
    );

    let mut rule = GridRule::life();
    rule.states[0].label = "Num states".to_owned();
    assert!(refusals(&rule)[0].message.contains("`NUM_STATES`"));

    // A reserved word of WGSL cannot name a state's constant.
    let mut rule = GridRule::life();
    rule.states[0].label = "Null".to_owned();
    let problems = rule
        .check_declarations()
        .err()
        .unwrap_or_else(|| panic!("a state named Null passed"));
    assert_eq!(problems.len(), 1, "{problems:#?}");
    assert_eq!(problems[0].0, Some(RuleDeclaration::State(0)));
    assert!(problems[0].1.message.contains("reserved word"), "{problems:#?}");
}

/// The `Params` block naga lays out holds a 4-byte field per parameter at the offsets the engine writes, and the
/// bytes the engine writes hold each value in its field.
#[test]
fn the_params_block_matches_the_naga_layout() {
    let mut rule = GridRule::life();
    rule.params.push(u32_param("radius", "Radius", 2, 1, 5));
    rule.params.push(bool_param("wrap", "Wrap", true));
    rule.params.push(choice_param("mode", "Mode", &["calm", "wild"], 1));
    let compiled = rule.compile().unwrap_or_else(|problems| panic!("{problems:#?}"));
    let (composition, _) = composed(&rule).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let module = &composition.module;
    let (handle, params) = module
        .types
        .iter()
        .find(|(_, ty)| ty.name.as_deref() == Some("Params"))
        .unwrap_or_else(|| panic!("no `Params` struct"));
    let naga::TypeInner::Struct { members, .. } = &params.inner else {
        panic!("`Params` is not a struct");
    };
    let mut layouter = naga::proc::Layouter::default();
    layouter
        .update(module.to_ctx())
        .unwrap_or_else(|error| panic!("{error}"));
    let names: Vec<&str> = members.iter().filter_map(|member| member.name.as_deref()).collect();
    assert_eq!(names, ["width", "height", "seed", "density", "radius", "wrap", "mode"]);
    let offsets: Vec<u32> = members.iter().map(|member| member.offset).collect();
    assert_eq!(offsets, [0, 4, 8, 12, 16, 20, 24]);
    let size = layouter[handle].size;
    assert_eq!(size, 28);

    let values = [
        ParamValue::U32(40),
        ParamValue::U32(30),
        ParamValue::F32(0.25),
        ParamValue::U32(4),
        ParamValue::Bool(true),
        ParamValue::Choice(1),
    ];
    let bytes = compiled.hooks().step_params_bytes(40, 30, &values, Some(9));
    assert_eq!(bytes.len(), 32, "the block is padded to 16 bytes");
    let word = |index: usize| {
        u32::from_le_bytes([
            bytes[4 * index],
            bytes[4 * index + 1],
            bytes[4 * index + 2],
            bytes[4 * index + 3],
        ])
    };
    assert_eq!([word(0), word(1)], [40, 30]);
    assert_eq!(f32::from_bits(word(3)), 0.25);
    assert_eq!([word(4), word(5), word(6)], [4, 1, 1]);
    // Another seed gives another word of the random stream.
    assert_ne!(
        compiled.hooks().step_params_bytes(40, 30, &values, Some(10))[8..12],
        bytes[8..12]
    );
}

/// Every pass of a rule binds at most 5 storage buffers, and the declarations the engine binds by name follow the
/// bindings of the composed shaders in order.
#[test]
fn a_grid_rule_binds_at_most_five_storage_buffers() {
    let compiled = GridRule::life()
        .compile()
        .unwrap_or_else(|problems| panic!("{problems:#?}"));
    let entry = compiled.entry(ModelCode::new(GridRule::CODE_FORMAT, "life"));
    let needs = entry.gpu_needs().unwrap_or_default();
    assert!(needs.storage_buffers() <= 5, "{needs:?}");

    let display = crate::gpu::runtime_grid::test_support::display_wgsl(&compiled);
    let reduce = crate::gpu::runtime_grid::test_support::reduce_wgsl(&compiled);
    for (text, declared) in [
        (compiled.step_wgsl(), STEP_BINDINGS),
        (display, DISPLAY_BINDINGS),
        (reduce, REDUCE_BINDINGS),
    ] {
        let module = naga::front::wgsl::parse_str(text).unwrap_or_else(|error| panic!("{error}"));
        let mut bound: Vec<(u32, String)> = module
            .global_variables
            .iter()
            .filter_map(|(_, global)| Some((global.binding.as_ref()?.binding, global.name.clone()?)))
            .collect();
        bound.sort();
        // naga's writer renames a global that another item's name shadows, as in `dims_1`.
        let names: Vec<&str> = bound
            .iter()
            .map(|(_, name)| {
                name.trim_end_matches(|c: char| c.is_ascii_digit())
                    .trim_end_matches('_')
            })
            .collect();
        let declared_names: Vec<&str> = declared.iter().map(|decl: &BindingDecl| decl.name).collect();
        assert_eq!(names, declared_names);
        let storage = declared.iter().filter(|decl| decl.kind.is_storage_buffer()).count();
        assert!(storage <= 5, "{declared_names:?}");
    }
}

/// A step that returns a state past the last one leaves every cell in the last state.
#[test]
fn a_result_past_the_last_state_is_clamped() {
    let Some(ctx) = headless_context("henad_runtime_grid_clamp", wgpu::Features::empty()) else {
        log::warn!("skipping a_result_past_the_last_state_is_clamped: no adapter");
        return;
    };
    let mut rule = GridRule::life();
    rule.states.push(RuleState::new("Ghost", [90, 90, 255, 255]));
    rule.step = format!("{STEP_HEAD}    return 7u;\n}}\n");
    let compiled = rule.compile().unwrap_or_else(|problems| panic!("{problems:#?}"));
    let entry = compiled.entry(ModelCode::new(GridRule::CODE_FORMAT, rule.step.clone()));
    let params = [ParamValue::U32(40), ParamValue::U32(24), ParamValue::F32(0.5)];
    let built = entry
        .build(&params, Some(3), Some(&ctx))
        .unwrap_or_else(|fault| panic!("{fault}"));
    let ModelState::Gpu(mut state) = built else {
        panic!("a rule builds a GPU state");
    };
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    state.encode_steps(&mut encoder, 2, None);
    state.encode_snapshot_passes(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));
    state.begin_stats_readback();
    state.poll_stats_readback(&ctx.device, true);
    let counts: Vec<f64> = state
        .stats()
        .iter()
        .map(|entry| match entry.value() {
            StatValue::Scalar(value) => *value,
            other => panic!("a rule's stat is a scalar, got {other:?}"),
        })
        .collect();
    assert_eq!(counts, [0.0, 0.0, 40.0 * 24.0]);
    assert_eq!(state.tick(), 2);
}

/// A rule that draws every cell's state afresh each tick draws from `pcg_hash(seed ^ pcg_hash(index ^ pcg_hash(tick)))`,
/// so the tick buffer advances and the stream differs from tick to tick. The Alive count of each tick equals the count
/// recomputed on the CPU from the seed word of the uniform block.
#[test]
fn a_random_rule_draws_a_fresh_stream_each_tick() {
    let Some(ctx) = headless_context("henad_runtime_grid_random", wgpu::Features::empty()) else {
        log::warn!("skipping a_random_rule_draws_a_fresh_stream_each_tick: no adapter");
        return;
    };
    let mut rule = GridRule::life();
    rule.step = format!(
        "#import henad::rng::{{next_bits, below}}\n{STEP_HEAD}    \
         return select(DEAD, ALIVE, below(next_bits(rng), 0x80000000u));\n}}\n"
    );
    let compiled = rule.compile().unwrap_or_else(|problems| panic!("{problems:#?}"));
    let entry = compiled.entry(ModelCode::new(GridRule::CODE_FORMAT, rule.step.clone()));
    let (width, height, seed) = (40u32, 24u32, Some(5));
    let params = [ParamValue::U32(width), ParamValue::U32(height), ParamValue::F32(0.3)];
    let uniform = compiled.hooks().step_params_bytes(width, height, &params, seed);
    let seed_word = u32::from_le_bytes([uniform[8], uniform[9], uniform[10], uniform[11]]);
    let built = entry
        .build(&params, seed, Some(&ctx))
        .unwrap_or_else(|fault| panic!("{fault}"));
    let ModelState::Gpu(mut state) = built else {
        panic!("a rule builds a GPU state");
    };
    let mut counts = Vec::new();
    for tick in 0..6u32 {
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_steps(&mut encoder, 1, None);
        state.encode_snapshot_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        state.begin_stats_readback();
        state.poll_stats_readback(&ctx.device, true);
        let alive = match state.stats().get(1).map(|entry| entry.value()) {
            Some(StatValue::Scalar(value)) => *value,
            other => panic!("no Alive count: {other:?}"),
        };
        let expected = (0..width * height)
            .filter(|&index| pcg_hash(pcg_hash(seed_word ^ pcg_hash(index ^ pcg_hash(tick)))) < 0x8000_0000)
            .count();
        assert_eq!(alive, expected as f64, "Alive after tick {tick}");
        counts.push(expected);
    }
    assert!(
        counts.windows(2).all(|pair| pair[0] != pair[1]),
        "the stream repeats from one tick to the next: {counts:?}"
    );
}
