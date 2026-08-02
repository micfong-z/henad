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

/// Henad's final grid against NetLogo's grid.
#[test]
fn gol_matches_netlogo_glider() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gol_glider_64x64.txt");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let fixture = parse_fixture(&text);

    let width = header_u32(&fixture, "width");
    let height = header_u32(&fixture, "height");
    let steps = header_u32(&fixture, "steps");

    assert_eq!(
        fixture.cells.len(),
        (width * height) as usize,
        "fixture has {} cells, header declares {width}x{height}",
        fixture.cells.len()
    );
    assert_eq!(fixture.width, width, "row length disagrees with the declared width");

    let start = grid_with(width, height, GLIDER, (0, 0));
    let ours = run(width, height, &start, steps);

    assert_eq!(
        render(&ours, width),
        render(&fixture.cells, width),
        "Henad and {} disagree after {steps} ticks",
        fixture
            .header
            .get("engine")
            .map_or("the reference engine", String::as_str)
    );
}
