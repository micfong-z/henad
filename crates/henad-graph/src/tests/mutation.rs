//! Neither the reader, the checker nor the host evaluator panics on a mutation corpus, and none runs long on a hostile
//! file.
//!
//! The corpus takes the graph corpus and every case file, applies random line and token edits from a fixed seed, and
//! adds generated files that push the region tree, the call graph and the cost arithmetic to their ends.

use std::panic::{AssertUnwindSafe, catch_unwind};

use henad_core::authoring::primitives::rng::xorshift64;

use super::case_files::case_file;
use super::corpus;
use crate::GraphFile;
use crate::catalogue::definitions;
use crate::host::{HostEvaluator, HostWorld, RunContext};

/// Values a token edit puts in place of a value.
const HOSTILE_VALUES: [&str; 24] = [
    "0",
    "-1",
    "65536",
    "65537",
    "4294967295",
    "4294967296",
    "9223372036854775807",
    "1e38",
    "-0.0",
    "\"\"",
    "\"n1\"",
    "\"n4294967295\"",
    "\"n1.k\"",
    "\"n1.prev.sum\"",
    "\"region.repeat\"",
    "\"region.put\"",
    "\"cat.transition\"",
    "\"fn.call\"",
    "[]",
    "[{ name = \"k\", type = \"u64\", combine = \"min\" }]",
    "{ u32 = 0 }",
    "{ param = \"steps\" }",
    "{ cat = \"health.Q\" }",
    "true",
];

/// Lines an edit inserts.
const HOSTILE_LINES: [&str; 8] = [
    "region = \"n1\"",
    "region = \"n2\"",
    "config.max = 65536",
    "config.acc = [{ name = \"sum\", type = \"u32\", combine = \"carry\" }]",
    "in.count = \"n1.k\"",
    "op = \"region.repeat\"",
    "[[pass.node]]\nid = \"n7777\"\nop = \"region.put\"\nconfig.acc = \"sum\"\nregion = \"n7777\"",
    "[[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"cells\"",
];

/// Returns a number below `bound` from `rng`.
fn below(rng: &mut u64, bound: usize) -> usize {
    *rng = xorshift64(*rng);
    (*rng >> 33) as usize % bound.max(1)
}

/// Returns `text` with one random edit.
fn mutate(text: &str, rng: &mut u64) -> String {
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
                let value = HOSTILE_VALUES[below(rng, HOSTILE_VALUES.len())];
                lines[at] = format!("{key} = {value}");
            }
        }
        5 => lines.insert(at, HOSTILE_LINES[below(rng, HOSTILE_LINES.len())].to_owned()),
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

/// Number of edited files the host evaluator has run.
static HOST_RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Reads `text`, checks it, holds the canonical text to a fixed point, and runs a file that checks on the host over a
/// small world. A panic fails the test with the text.
fn exercise(text: &str) -> bool {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let Ok(file) = GraphFile::parse(text) else { return false };
        let report = file.check();
        for diagnostic in &report.diagnostics {
            assert!(!diagnostic.message.is_empty());
        }
        if !report.has_errors()
            && let Ok(evaluator) = HostEvaluator::new(&file)
        {
            let world = match HostWorld::of(evaluator.document()) {
                HostWorld::Grid { boundary, .. } => HostWorld::Grid {
                    width: 4,
                    height: 3,
                    boundary,
                },
                _ => HostWorld::Agents {
                    count: 12,
                    width: 6.0,
                    height: 5.0,
                },
            };
            HOST_RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if let Ok(mut lanes) = evaluator.init(world, &[], Some(1)) {
                for pass in &evaluator.document().passes {
                    let context = RunContext::tick(Some(1), 2);
                    // A fault is an outcome. A panic is the failure.
                    let _faulted = evaluator.run_pass(&pass.id, &mut lanes, &[], &context, &mut 1).is_err();
                }
            }
        }
        let canonical = file.to_toml();
        if let Ok(again) = GraphFile::parse(&canonical) {
            assert_eq!(again.to_toml(), canonical, "the canonical text is no fixed point");
            again.check();
        }
        true
    }));
    outcome.unwrap_or_else(|_| panic!("the reader, the checker or the host evaluator panicked on:\n{text}"))
}

#[test]
fn the_reader_and_checker_never_panic_on_the_mutation_corpus() {
    let mut seeds: Vec<(String, usize)> = corpus::all().into_iter().map(|(_, text)| (text, 300)).collect();
    for def in definitions() {
        for case in def.cases {
            seeds.push((case_file(def, case), 12));
        }
    }
    let mut rng = 0x6D75_7461_7465_0001u64;
    let (mut cases, mut read) = (0usize, 0usize);
    for (seed, edits) in &seeds {
        for edit in 0..*edits {
            let mut text = mutate(seed, &mut rng);
            if edit % 3 == 0 {
                text = mutate(&text, &mut rng);
            }
            cases += 1;
            read += usize::from(exercise(&text));
        }
    }
    assert!(
        read * 8 > cases,
        "only {read} of {cases} edits read, so the corpus tests the reader alone"
    );
    let host_runs = HOST_RUNS.load(std::sync::atomic::Ordering::Relaxed);
    assert!(host_runs > 100, "only {host_runs} edits reach the host evaluator");
}

/// Returns a grid file whose tick pass holds `nodes`, and `extra` tables after it.
fn grid_with(nodes: &str, extra: &str) -> String {
    format!(
        "format = \"henad-model\"\nformat_version = 1\nkind = \"graph\"\nid = \"node_hostile\"\n\n[graph]\ntopology = \"grid\"\nlibrary = 1\n\n[[target]]\nbackend = \"gpu\"\n\n[[lane]]\nid = \"x\"\ndomain = \"cells\"\ntype = \"u32\"\n{extra}\n[[pass]]\nid = \"step\"\nstage = \"tick\"\nkind = \"map\"\ndomain = \"cells\"\n{nodes}"
    )
}

#[test]
fn a_hostile_region_tree_or_call_graph_is_reported_without_a_panic() {
    let started = std::time::Instant::now();
    // A chain of 4000 nested Repeats, each at the largest bound.
    let mut chain = String::new();
    for id in 1..=4000u32 {
        let region = if id == 1 {
            String::new()
        } else {
            format!("region = \"n{}\"\n", id - 1)
        };
        chain.push_str(&format!(
            "\n[[pass.node]]\nid = \"n{id}\"\nop = \"region.repeat\"\nconfig.max = 65536\nconfig.acc = []\nin.count = {{ u32 = 1 }}\n{region}"
        ));
    }
    let report = GraphFile::parse(&grid_with(&chain, ""))
        .expect("the chain reads")
        .check();
    assert_eq!(report.passes[0].cost, u64::MAX);
    assert!(report.has_errors());

    // The same chain closed into a cycle of regions.
    let cycle = chain.replacen(
        "in.count = { u32 = 1 }\n",
        "in.count = { u32 = 1 }\nregion = \"n4000\"\n",
        1,
    );
    let report = GraphFile::parse(&grid_with(&cycle, ""))
        .expect("the cycle reads")
        .check();
    assert!(report.errors().any(|error| error.message.contains("form a cycle")));

    // A chain of 4000 additions feeding a count, deeper than the bound follows.
    let mut sums =
        String::from("\n[[pass.node]]\nid = \"n1\"\nop = \"math.add\"\nin.a = { u32 = 1 }\nin.b = { u32 = 1 }\n");
    for id in 2..=4000u32 {
        sums.push_str(&format!(
            "\n[[pass.node]]\nid = \"n{id}\"\nop = \"math.add\"\nin.a = \"n{}\"\nin.b = \"n{}\"\n",
            id - 1,
            id - 1
        ));
    }
    sums.push_str("\n[[pass.node]]\nid = \"n5000\"\nop = \"region.repeat\"\nconfig.max = 2\nconfig.acc = []\nin.count = \"n4000\"\n");
    GraphFile::parse(&grid_with(&sums, "")).expect("the sums read").check();

    // 128 functions, each calling the next twice with a Repeat beside the calls.
    let mut functions = String::new();
    for index in 0..128u32 {
        functions.push_str(&format!("\n[[function]]\nid = \"f{index}\"\nkind = \"pure\"\n"));
        for call in 0..2u32 {
            functions.push_str(&format!(
                "\n[[function.node]]\nid = \"n{}\"\nop = \"fn.call\"\nconfig.function = \"f{}\"\nconfig.key = {}\n",
                10_000 + 4 * index + call,
                (index + 1) % 128,
                10_000 + 4 * index + call
            ));
        }
        functions.push_str(&format!(
            "\n[[function.node]]\nid = \"n{}\"\nop = \"region.repeat\"\nconfig.max = 65536\nconfig.acc = []\nin.count = {{ u32 = 1 }}\n",
            10_002 + 4 * index
        ));
    }
    let call = "\n[[pass.node]]\nid = \"n1\"\nop = \"fn.call\"\nconfig.function = \"f0\"\nconfig.key = 1\n";
    let report = GraphFile::parse(&grid_with(call, &functions))
        .expect("the functions read")
        .check();
    assert_eq!(report.passes[0].cost, u64::MAX);
    let open_chain = functions.replace("config.function = \"f0\"", "config.function = \"none\"");
    let report = GraphFile::parse(&grid_with(call, &open_chain))
        .expect("the chain reads")
        .check();
    assert!(
        report
            .errors()
            .any(|error| error.message.contains("deeper than 8 functions"))
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(30),
        "the hostile files took {:?}",
        started.elapsed()
    );
}
