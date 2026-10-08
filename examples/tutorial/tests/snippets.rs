//! Checks the code the first-model pages show against the finished files under `src/`.
//!
//! Every fenced block titled with a path under `src/` is checked line by line: each line, with its annotation marker
//! (`// (1)!`) removed, has to appear as a line of the file named in the title. Blocks titled `src/lib.rs` refer to
//! the template's file, which this crate does not hold, and are left out. A page that grows a function shows a first
//! version first, and [`FIRST_VERSIONS`] lists the lines of each first version.
//!
//! The template's `src/lib.rs` sets the same crate attributes as this crate's `src/lib.rs`, each under the same
//! comment. A reader's crate is a copy of the template, and the code that the pages show needs those attributes there
//! too.

use std::path::{Path, PathBuf};

/// The five pages, under `docs/guide/first-model/`.
const PAGES: [&str; 5] = [
    "game-of-life.md",
    "ants.md",
    "virus-network.md",
    "gpu-game-of-life.md",
    "gpu-ants.md",
];

/// Lines a page shows in a first version that a later block on the same page replaces, as `(page, line)`.
///
/// Each line is trimmed, as the check reads it.
const FIRST_VERSIONS: [(&str, &str); 22] = [
    ("game-of-life.md", "fn step_cell(cell: u8, neighbors: &[u8]) -> u8 {"),
    ("game-of-life.md", "impl GridModel for LifeModel {}"),
    ("game-of-life.md", "const PALETTE: [[u8; 4]; 2] = ["),
    (
        "game-of-life.md",
        "fn init(grid: &mut Grid2D<u8>, _params: &[ParamValue], rng: &mut u64) {",
    ),
    ("game-of-life.md", "let threshold = (0.3 * u32::MAX as f32) as u32;"),
    ("game-of-life.md", "const STATS: &'static [StatDescriptor] = &[];"),
    ("game-of-life.md", "fn stats(_grid: &Grid2D<u8>) -> Vec<StatValue> {"),
    ("game-of-life.md", "Vec::new()"),
    ("ants.md", "impl AgentModel for ForagingModel {}"),
    ("virus-network.md", "impl NetworkModel for VirusModel {}"),
    ("virus-network.md", "nodes.graph.update_colors(|src, dst, color| {"),
    ("virus-network.md", "let mut changed = false;"),
    ("virus-network.md", "for e in 0..color.len() {"),
    (
        "virus-network.md",
        "let wanted = edge_color(state[src[e] as usize], state[dst[e] as usize]);",
    ),
    ("virus-network.md", "if color[e] != wanted {"),
    ("virus-network.md", "color[e] = wanted;"),
    ("virus-network.md", "changed = true;"),
    ("virus-network.md", "changed"),
    ("gpu-game-of-life.md", "impl GpuGridModel for GpuLifeModel {}"),
    (
        "gpu-game-of-life.md",
        "fn seed_buffers(width: u32, height: u32, _params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {",
    ),
    ("gpu-game-of-life.md", "vec![seed_random(width, height, 0.3, rng)]"),
    ("gpu-ants.md", "impl GpuAgentModel for GpuForagingModel {}"),
];

/// One fenced block titled with a path under `src/`.
struct Snippet {
    /// Path named in the title, relative to the crate root.
    path: String,
    /// One-based line number of the block's first line of code on its page.
    first_line: usize,
    lines: Vec<String>,
}

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

/// Returns `line` without a trailing annotation marker such as `// (3)!`.
fn strip_annotation(line: &str) -> &str {
    let Some(start) = line.rfind("// (") else {
        return line;
    };
    let marker = &line[start + 4..];
    let is_marker = marker
        .strip_suffix(")!")
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()));
    if is_marker { line[..start].trim_end() } else { line }
}

/// Returns every fenced block on `page` titled with a path under `src/`, other than `src/lib.rs`.
fn snippets(page: &str) -> Vec<Snippet> {
    let mut found = Vec::new();
    let mut lines = page.lines().enumerate();
    while let Some((_, line)) = lines.next() {
        let trimmed = line.trim_start();
        let Some(info) = trimmed.strip_prefix("```") else {
            continue;
        };
        let indent = &line[..line.len() - trimmed.len()];
        let mut body = Vec::new();
        let mut first_line = 0;
        for (number, line) in lines.by_ref() {
            if line.trim() == "```" {
                break;
            }
            if body.is_empty() {
                first_line = number + 1;
            }
            body.push(line.strip_prefix(indent).unwrap_or(line).to_owned());
        }
        let title = info
            .split_once("title=\"")
            .and_then(|(_, rest)| rest.split_once('"'))
            .map(|(title, _)| title);
        if let Some(path) = title.filter(|path| path.starts_with("src/") && *path != "src/lib.rs") {
            found.push(Snippet {
                path: path.to_owned(),
                first_line,
                lines: body,
            });
        }
    }
    found
}

/// Returns each inner attribute of a crate root, joined to the `//` comment lines directly above it.
fn crate_attributes(source: &str) -> Vec<String> {
    let mut attributes = Vec::new();
    let mut attribute_lines = Vec::new();
    for line in source.lines() {
        if line.starts_with("#![") {
            attribute_lines.push(line);
            attributes.push(attribute_lines.join("\n"));
            attribute_lines.clear();
        } else if line.starts_with("//") && !line.starts_with("//!") {
            attribute_lines.push(line);
        } else {
            attribute_lines.clear();
        }
    }
    attributes
}

#[test]
fn every_page_snippet_matches_its_finished_file() {
    let mut drifted = Vec::new();
    let mut first_versions_seen = [0usize; FIRST_VERSIONS.len()];

    for page_name in PAGES {
        let page = read(&crate_root().join("../../docs/guide/first-model").join(page_name));
        let blocks = snippets(&page);
        assert!(
            !blocks.is_empty(),
            "{page_name} shows no block titled with a file under src/"
        );

        for block in blocks {
            let file = read(&crate_root().join(&block.path));
            for (offset, line) in block.lines.iter().enumerate() {
                let line = strip_annotation(line).trim();
                if line.is_empty() || file.lines().any(|candidate| candidate.trim() == line) {
                    continue;
                }
                let first_version = FIRST_VERSIONS
                    .iter()
                    .position(|&(page, text)| page == page_name && text == line);
                match first_version {
                    Some(index) => first_versions_seen[index] += 1,
                    None => drifted.push(format!(
                        "{page_name}:{}: `{line}` is not a line of {}",
                        block.first_line + offset,
                        block.path
                    )),
                }
            }
        }
    }

    assert!(
        drifted.is_empty(),
        "the pages show code their finished files no longer hold:\n{}",
        drifted.join("\n")
    );
    let stale: Vec<_> = FIRST_VERSIONS
        .iter()
        .zip(first_versions_seen)
        .filter(|&(_, seen)| seen == 0)
        .map(|(&(page, line), _)| format!("{page}: `{line}`"))
        .collect();
    assert!(
        stale.is_empty(),
        "FIRST_VERSIONS lists lines no page shows any more:\n{}",
        stale.join("\n")
    );
}

#[test]
fn the_template_sets_the_crate_attributes_this_crate_sets() {
    let tutorial = read(&crate_root().join("src/lib.rs"));
    let template = read(&crate_root().join("../../templates/model-project/src/lib.rs"));
    assert_eq!(
        crate_attributes(&template),
        crate_attributes(&tutorial),
        "the template's src/lib.rs and this crate's set different crate attributes"
    );
}

#[test]
fn an_annotation_marker_is_stripped() {
    assert_eq!(strip_annotation("let x = 1; // (12)!"), "let x = 1;");
    assert_eq!(strip_annotation("// (a)!"), "// (a)!");
    assert_eq!(strip_annotation("let y = 2; // a comment"), "let y = 2; // a comment");
}
