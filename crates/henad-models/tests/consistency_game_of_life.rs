//! Cross-engine and self consistency check for Game of Life.
//!
//! See `tests/fixtures/docs/game_of_life_fixture.md` for how a fixture is produced.

use std::collections::HashMap;
use std::path::Path;

use henad_compute::grid_engine::GridModelState;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_models::game_of_life::GameOfLifeModel;

/// Standard glider, as `(x, y)` offsets in buffer order with row 0 first.
///
/// ```text
/// .X.
/// ..X
/// XXX
/// ```
const GLIDER: &[(u32, u32)] = &[(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];

/// R-pentomino, in the same coordinates.
///
/// ```text
/// .XX
/// XX.
/// .X.
/// ```
const R_PENTOMINO: &[(u32, u32)] = &[(1, 0), (2, 0), (0, 1), (1, 1), (1, 2)];

/// Resolves a fixture's `scenario` header to the pattern it started from.
fn pattern(scenario: &str) -> &'static [(u32, u32)] {
    match scenario.split_whitespace().next().unwrap_or_default() {
        "glider" => GLIDER,
        "r-pentomino" => R_PENTOMINO,
        other => panic!("fixture declares unknown scenario `{other}`"),
    }
}

fn params(width: u32, height: u32) -> Vec<ParamValue> {
    vec![ParamValue::U32(width), ParamValue::U32(height), ParamValue::F32(0.0)]
}

fn grid_with(width: u32, height: u32, live: &[(u32, u32)], at: (u32, u32)) -> Vec<u8> {
    let mut cells = vec![0u8; width as usize * height as usize];
    for (dx, dy) in live {
        let x = (at.0 + dx) % width;
        let y = (at.1 + dy) % height;
        cells[(y * width + x) as usize] = 1;
    }
    cells
}

fn run(width: u32, height: u32, cells: &[u8], steps: u32) -> Vec<u8> {
    let p = params(width, height);
    let mut state =
        GridModelState::<GameOfLifeModel>::from_cells(&p, cells).expect("cell buffer matches the declared grid size");
    for _ in 0..steps {
        state.step();
    }
    let view = state.grid_view().expect("grid model exposes a grid view");
    view.cells.to_vec()
}

/// Renders a grid as rows of `.`/`X` for readability.
fn render(cells: &[u8], width: u32) -> String {
    cells
        .chunks(width as usize)
        .map(|row| row.iter().map(|&c| if c == 1 { 'X' } else { '.' }).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

struct Fixture {
    header: HashMap<String, String>,
    cells: Vec<u8>,
    width: u32,
}

/// Parses a fixture of `# key: value` header lines and rows of `0`/`1`.
///
/// `width`, `height` and `steps` keys are used in the comparison.
fn parse_fixture(text: &str) -> Fixture {
    let mut header = HashMap::new();
    let mut rows: Vec<&str> = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            if let Some((k, v)) = rest.split_once(':') {
                header.insert(k.trim().to_lowercase(), v.trim().to_owned());
            }
            continue;
        }
        rows.push(line);
    }

    let width = rows.first().map_or(0, |r| r.len()) as u32;
    assert!(width > 0, "fixture has no grid rows");
    assert!(rows.iter().all(|r| r.len() as u32 == width), "fixture rows are ragged");

    let cells = rows
        .iter()
        .flat_map(|r| r.bytes())
        .map(|b| match b {
            b'0' => 0u8,
            b'1' => 1u8,
            other => panic!("unexpected cell byte {:?} in fixture", other as char),
        })
        .collect();

    Fixture { header, cells, width }
}

fn header_u32(f: &Fixture, key: &str) -> u32 {
    f.header
        .get(key)
        .unwrap_or_else(|| panic!("fixture header is missing `{key}`"))
        .parse()
        .unwrap_or_else(|_| panic!("fixture header `{key}` is not a number"))
}

/// A glider on a square torus translates by (1,1) every 4 ticks, so on a W-wide world it is back
/// exactly where it started after 4W ticks.
///
/// This is a self-consistency check.
#[test]
fn glider_returns_to_origin_after_full_wrap() {
    const W: u32 = 64;
    let start = grid_with(W, W, GLIDER, (0, 0));
    let end = run(W, W, &start, 4 * W);

    assert_eq!(
        render(&end, W),
        render(&start, W),
        "glider did not return to its starting cells after {} ticks",
        4 * W
    );
}

/// Henad's final grid against every reference.
#[test]
fn matches_every_reference_fixture() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/game_of_life");
    let mut checked = Vec::new();

    let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_none_or(|e| e != "txt") {
            continue;
        }

        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {name}: {e}"));
        let fixture = parse_fixture(&text);

        let width = header_u32(&fixture, "width");
        let height = header_u32(&fixture, "height");
        let steps = header_u32(&fixture, "steps");
        let engine = fixture.header.get("engine").map_or("?", String::as_str);
        let scenario = fixture.header.get("scenario").map_or("?", String::as_str);

        // Truncated file guard
        assert_eq!(
            fixture.cells.len(),
            (width * height) as usize,
            "{name} has {} cells, header declares {width}x{height}",
            fixture.cells.len()
        );
        assert_eq!(fixture.width, width, "{name}: row length disagrees with declared width");

        let start = grid_with(width, height, pattern(scenario), (0, 0));
        let ours = run(width, height, &start, steps);

        assert_eq!(
            render(&ours, width),
            render(&fixture.cells, width),
            "Henad and {engine} disagree on {scenario} after {steps} ticks ({name})"
        );
        checked.push(format!("{engine} / {scenario}"));
    }

    // In case some unexpected renaming or deletion happen.
    assert!(!checked.is_empty(), "no fixtures found in {}", dir.display());
}
