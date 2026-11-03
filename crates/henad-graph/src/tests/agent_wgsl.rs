//! Tests of the WGSL compiler of agents graphs that need no device: the budgets and memory limits its spec reports,
//! the replay flag, and the refusals of what the engines cannot run.

use henad_compute::gpu::wgpu;
use henad_core::params::ParamValue;

use crate::GraphFile;
use crate::wgsl::{AgentProgram, GridProgram, ShaderRole};

/// Returns a node `id` of kind `op` with the further lines `lines`.
fn node(id: usize, op: &str, lines: &str) -> String {
    format!("\n[[pass.node]]\nid = \"n{id}\"\nop = \"{op}\"\n{lines}")
}

/// Returns the file `id` of an agents world of 1000 agents, at most 1000000, with a position lane, then
/// `declarations` and `passes`.
fn agents_file(id: &str, declarations: &str, passes: &str) -> GraphFile {
    let text = format!(
        "format = \"henad-model\"\nformat_version = 1\nkind = \"graph\"\nid = \"{id}\"\n\n\
         [graph]\ntopology = \"agents\"\nlibrary = 1\n\n[[target]]\nbackend = \"gpu\"\n\n\
         [agents]\ncount = 1000\nmax_count = 1000000\nwidth = 100.0\nheight = 100.0\n\n\
         [[lane]]\nid = \"pos\"\ndomain = \"agents\"\ntype = \"vec2f\"\nrole = \"position\"\n\
         {declarations}{passes}"
    );
    GraphFile::parse(&text).unwrap_or_else(|error| panic!("{id} reads: {error}"))
}

/// Returns the shortfalls of `file` on a baseline device with `agents` agents, after it compiles.
fn baseline_shortfalls(file: &GraphFile, agents: u32) -> Vec<String> {
    let program = AgentProgram::compile(file).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let limits = wgpu::Limits::default();
    let params = [ParamValue::U32(agents), ParamValue::F32(100.0), ParamValue::F32(100.0)];
    program.spec().demand(&params, &limits).shortfalls(&limits)
}

/// Returns a file whose stats pass counts a category of 256 variants with `counts` nodes, 1 KiB of tallies each.
fn tally_flood(counts: usize) -> GraphFile {
    let variants: Vec<String> = (0..256)
        .map(|k| format!("{{ name = \"V{k}\", label = \"V{k}\", color = [{k}, 0, 0, 255] }}"))
        .collect();
    let declarations = format!(
        "\n[[category]]\nid = \"big\"\nvariants = [{}]\n\n\
         [[lane]]\nid = \"c\"\ndomain = \"agents\"\ntype = \"cat:big\"\ninitial = {{ cat = \"big.V0\" }}\n\n\
         [[stat]]\nlabel = \"S0\"\nfrom = \"n2.V0\"\ncolor = [1, 1, 1, 255]\n",
        variants.join(", ")
    );
    let mut passes =
        String::from("\n[[pass]]\nid = \"counts\"\nstage = \"stats\"\nkind = \"map\"\ndomain = \"agents\"\n");
    passes.push_str(&node(1, "lane.get", "config.lane = \"c\"\n"));
    for k in 0..counts {
        passes.push_str(&node(
            k + 2,
            "stat.count_by",
            "config.category = \"big\"\nin.value = \"n1\"\n",
        ));
    }
    agents_file("node_tally_flood", &declarations, &passes)
}

/// The reduce leaf of an agents world counts its tallies in workgroup memory, and a leaf past the baseline's 16 KiB
/// is refused with a line naming it.
#[test]
fn agent_tallies_past_the_workgroup_memory_are_refused() {
    assert!(
        baseline_shortfalls(&tally_flood(16), 1000).is_empty(),
        "16 KiB of tallies fit"
    );
    let found = baseline_shortfalls(&tally_flood(17), 1000);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("'node_tally_flood_reduce_leaf'") && found[0].contains("17408 bytes of workgroup memory"),
        "{}",
        found[0]
    );
}

/// Returns a file whose tick pass sums `powers` integer powers of 64 entries' tables each, per agent.
fn powi_flood(powers: usize) -> GraphFile {
    let declarations = "\n[[lane]]\nid = \"x\"\ndomain = \"agents\"\ntype = \"f32\"\ninitial = { f32 = 0.0 }\n";
    let mut passes = String::from("\n[[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"agents\"\n");
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
    agents_file("node_powi_flood", declarations, &passes)
}

/// A pass of an agents world whose uniform block passes the baseline's 64 KiB is refused with a line naming it.
#[test]
fn an_agent_uniform_block_past_the_binding_limit_is_refused() {
    assert!(baseline_shortfalls(&powi_flood(200), 1000).is_empty(), "200 tables fit");
    let found = baseline_shortfalls(&powi_flood(300), 1000);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("'node_powi_flood_step'") && found[0].contains("uniform block of"),
        "{}",
        found[0]
    );
}

/// An agents file within the per-element limit is refused by the submission budget at a large population, before
/// anything is built, with a line naming its heaviest pass. Each agent runs a Repeat of 16384 iterations.
#[test]
fn an_agent_graph_past_the_submission_budget_is_refused_at_a_large_population() {
    let declarations = "\n[[lane]]\nid = \"x\"\ndomain = \"agents\"\ntype = \"u32\"\n";
    let mut passes =
        String::from("\n[[pass]]\nid = \"churn\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"agents\"\n");
    passes.push_str(&node(
        1,
        "region.repeat",
        "config.max = 16384\nconfig.acc = [{ name = \"churned\", type = \"u32\", combine = \"sum\" }]\n\
         in.count = { u32 = 16384 }\n",
    ));
    passes.push_str(&node(
        2,
        "region.put",
        "region = \"n1\"\nconfig.acc = \"churned\"\nin.value = \"n1.k\"\n",
    ));
    passes.push_str(&node(3, "lane.set", "config.lane = \"x\"\nin.value = \"n1.churned\"\n"));
    let file = agents_file("node_churn", declarations, &passes);
    assert!(!file.check().has_errors(), "{:#?}", file.check().diagnostics);
    assert!(baseline_shortfalls(&file, 1000).is_empty(), "a thousand agents fit");
    let found = baseline_shortfalls(&file, 1_000_000);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("units of work") && found[0].contains("'node_churn_churn'"),
        "{}",
        found[0]
    );
}

/// A graph whose radius fold holds a `carry`, sums floats, or reads its `k` or a `prev.<acc>` declares that it does
/// not replay, and one whose fold only counts, or that has no fold, replays.
#[test]
fn the_replay_flag_follows_the_radius_folds() {
    for (name, text) in super::corpus::all() {
        let file = GraphFile::parse(&text).unwrap_or_else(|error| panic!("{name}: {error}"));
        if !matches!(file.document().world, crate::document::World::Agents(_)) {
            continue;
        }
        let program = AgentProgram::compile(&file).unwrap_or_else(|problems| panic!("{name}: {problems:#?}"));
        let expected = !name.contains("boids");
        assert_eq!(program.replays_exactly(), expected, "{name}");
    }
    let fold = |accumulator: &str, combine: &str, value: &str| {
        let zero = if accumulator == "f32" { "0.0" } else { "0" };
        let declarations = format!("\n[[lane]]\nid = \"n\"\ndomain = \"agents\"\ntype = \"{accumulator}\"\n");
        let mut passes =
            String::from("\n[[pass]]\nid = \"look\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"agents\"\n");
        passes.push_str(&node(1, "lane.get", "config.lane = \"pos\"\n"));
        passes.push_str(&node(
            2,
            "agent.radius_fold",
            &format!(
                "config.acc = [{{ name = \"a\", type = \"{accumulator}\", combine = \"{combine}\" }}]\nin.center = \
                 \"n1\"\nin.radius = {{ f32 = 5.0 }}\nin.cell_size = {{ f32 = 5.0 }}\nin.init.a = {{ {accumulator} = \
                 {zero} }}\n"
            ),
        ));
        passes.push_str(&node(
            3,
            "region.put",
            &format!("region = \"n2\"\nconfig.acc = \"a\"\nin.value = {value}\n"),
        ));
        passes.push_str(&node(4, "lane.set", "config.lane = \"n\"\nin.value = \"n2.a\"\n"));
        let file = agents_file("node_look", &declarations, &passes);
        AgentProgram::compile(&file)
            .unwrap_or_else(|problems| panic!("{problems:#?}"))
            .replays_exactly()
    };
    assert!(fold("u32", "count", "{ u32 = 1 }"), "an integer count replays");
    assert!(fold("f32", "max", "\"n2.dist_sq\""), "a float maximum replays");
    assert!(!fold("u32", "sum", "\"n2.k\""), "a sum of `k` depends on the order");
    assert!(
        !fold("f32", "sum", "\"n2.dist_sq\""),
        "a float sum depends on the order"
    );
    assert!(!fold("f32", "carry", "\"n2.dist_sq\""), "a carry depends on the order");

    // The squared distance of the first agent the walk visits: a float maximum gated by a count's `prev`.
    let declarations = "\n[[lane]]\nid = \"n\"\ndomain = \"agents\"\ntype = \"f32\"\n";
    let mut passes = String::from("\n[[pass]]\nid = \"look\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"agents\"\n");
    passes.push_str(&node(1, "lane.get", "config.lane = \"pos\"\n"));
    passes.push_str(&node(
        2,
        "agent.radius_fold",
        "config.acc = [{ name = \"seen\", type = \"u32\", combine = \"count\" }, { name = \"first\", type = \
         \"f32\", combine = \"max\" }]\nin.center = \"n1\"\nin.radius = { f32 = 5.0 }\nin.cell_size = { f32 = 5.0 }\n\
         in.init.first = { f32 = 0.0 }\n",
    ));
    passes.push_str(&node(3, "region.put", "region = \"n2\"\nconfig.acc = \"seen\"\n"));
    passes.push_str(&node(
        4,
        "cmp.eq",
        "region = \"n2\"\nin.a = \"n2.prev.seen\"\nin.b = { u32 = 0 }\n",
    ));
    passes.push_str(&node(
        5,
        "logic.select",
        "region = \"n2\"\nin.cond = \"n4\"\nin.if_true = \"n2.dist_sq\"\nin.if_false = { f32 = 0.0 }\n",
    ));
    passes.push_str(&node(
        6,
        "region.put",
        "region = \"n2\"\nconfig.acc = \"first\"\nin.value = \"n5\"\n",
    ));
    passes.push_str(&node(7, "lane.set", "config.lane = \"n\"\nin.value = \"n2.first\"\n"));
    let file = agents_file("node_first", declarations, &passes);
    let program = AgentProgram::compile(&file).unwrap_or_else(|problems| panic!("{problems:#?}"));
    assert!(!program.replays_exactly(), "a read of `prev` depends on the order");
}

/// A stats pass of a grid that sums floats compiles to a reduce shader that binds `partials` and folds each lane with
/// `block_sum`, and the spec declares one summed lane.
#[test]
fn a_float_sum_on_a_grid_writes_partials() {
    let text = "format = \"henad-model\"\nformat_version = 1\nkind = \"graph\"\nid = \"node_sums\"\n\n[graph]\n\
                topology = \"grid\"\nlibrary = 1\n\n[[target]]\nbackend = \"gpu\"\n\n[grid]\nwidth = 8\nheight = 8\n\
                boundary = \"torus\"\n\n[[lane]]\nid = \"x\"\ndomain = \"cells\"\ntype = \"f32\"\n\n[[stat]]\n\
                label = \"Total\"\nfrom = \"n2\"\ncolor = [1, 1, 1, 255]\n\n[[pass]]\nid = \"totals\"\n\
                stage = \"stats\"\nkind = \"map\"\ndomain = \"cells\"\n\n[[pass.node]]\nid = \"n1\"\n\
                op = \"lane.get\"\nconfig.lane = \"x\"\n\n[[pass.node]]\nid = \"n2\"\nop = \"stat.sum\"\n\
                in.value = \"n1\"\n\n[[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\n\
                domain = \"cells\"\n\n[[pass.node]]\nid = \"n3\"\nop = \"lane.get\"\nconfig.lane = \"x\"\n\n\
                [[pass.node]]\nid = \"n4\"\nop = \"lane.set\"\nconfig.lane = \"x\"\nin.value = \"n3\"\n";
    let file = GraphFile::parse(text).expect("the file reads");
    assert!(!file.check().has_errors(), "{:#?}", file.check().diagnostics);
    let program = GridProgram::compile(&file).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let reduce = program
        .shaders()
        .iter()
        .find(|shader| shader.role() == &ShaderRole::Reduce)
        .expect("a reduce shader");
    for expected in [
        "#import henad::reduce_tree::block_sum",
        "var<storage, read_write> partials: array<f32>",
        "henad_sum_0 = henad_sum_0 + ",
        "let henad_total_0 = block_sum(henad_local, henad_sum_0);",
        "partials[henad_block * 1u + 0u] = henad_total_0;",
    ] {
        assert!(reduce.text().contains(expected), "{expected}\n{}", reduce.text());
    }
    let spec = program.spec();
    assert_eq!(spec.sums, 1);
    assert_eq!(
        spec.reduce.workgroup_bytes,
        4 + 256 * 4,
        "the counter and the scratch of `block_sum`"
    );
}

/// Returns a file whose function `widen` holds ten expressions of 49 additions each, called `calls` times in a chain by
/// each of `passes` tick passes.
fn widened(passes: usize, calls: usize) -> GraphFile {
    let sum = format!("o = a{}", " + a".repeat(49));
    let mut declarations = String::from(
        "\n[[lane]]\nid = \"v\"\ndomain = \"agents\"\ntype = \"f32\"\n\n[[function]]\nid = \"widen\"\nkind = \"pure\"\n\n\
         [[function.input]]\nport = \"a\"\ntype = \"f32\"\n\n[[function.output]]\nport = \"o\"\ntype = \"f32\"\n\n\
         [[function.node]]\nid = \"n1\"\nop = \"fn.input\"\nconfig.port = \"a\"\n",
    );
    for k in 0..10 {
        let source = if k == 0 {
            "n1".to_owned()
        } else {
            format!("n{}.o", 1 + k)
        };
        declarations.push_str(&format!(
            "\n[[function.node]]\nid = \"n{}\"\nop = \"math.expr\"\nconfig.text = \"{sum}\"\n\
             config.inputs = [{{ name = \"a\", type = \"f32\" }}]\nconfig.outputs = [\"o\"]\nin.a = \"{source}\"\n",
            2 + k
        ));
    }
    declarations
        .push_str("\n[[function.node]]\nid = \"n12\"\nop = \"fn.output\"\nconfig.port = \"o\"\nin.value = \"n11.o\"\n");
    let mut text = String::new();
    let mut next = 100;
    for pass in 0..passes {
        text.push_str(&format!(
            "\n[[pass]]\nid = \"widen_{pass}\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"agents\"\n"
        ));
        text.push_str(&node(next, "lane.get", "config.lane = \"v\"\n"));
        for call in 0..calls {
            text.push_str(&node(
                next + 1 + call,
                "fn.call",
                &format!(
                    "config.function = \"widen\"\nconfig.key = {}\nin.a = \"n{}\"\n",
                    1 + pass * calls + call,
                    next + call
                ),
            ));
        }
        text.push_str(&node(
            next + calls + 1,
            "lane.set",
            &format!("config.lane = \"v\"\nin.value = \"n{}\"\n", next + calls),
        ));
        next += calls + 2;
    }
    agents_file("node_widened", &declarations, &text)
}

/// A file within the inlined node limit compiles although its normalised text passes the size of a file, and the
/// nodes its expressions expand into count toward that limit, before anything is inlined.
#[test]
fn expansions_count_toward_the_inlined_limit_and_no_text_limit_refuses_them() {
    let file = widened(1, 35);
    assert!(file.text().len() < 64 * 1024, "{} bytes", file.text().len());
    assert!(!file.check().has_errors(), "{:#?}", file.check().diagnostics);
    let normalised = crate::host::HostEvaluator::new(&file).expect("the file runs on the host");
    let bytes = crate::file::canonical_text(normalised.document(), None).len();
    assert!(bytes > crate::limits::MAX_FILE_BYTES, "{bytes} bytes once normalised");
    let program = AgentProgram::compile(&file).unwrap_or_else(|problems| panic!("{problems:#?}"));
    assert!(program.replays_exactly());

    let file = widened(2, 100);
    let report = file.check();
    assert!(
        report.errors().any(|error| error
            .message
            .contains("after its function calls and expressions are inlined")),
        "{:#?}",
        report.diagnostics
    );
}
