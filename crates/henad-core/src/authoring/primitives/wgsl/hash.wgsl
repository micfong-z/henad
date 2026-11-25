#define_import_path henad::hash
#import henad::space::wrap_index

// The walk over a neighbour index that a radius query takes, the twin of `SpatialHash::for_each_within`.
//
// A walk covers the cells within `radius` of a point, each as its slice `sorted[cell_start[c]..cell_start[c + 1]]`
// of the index the engine rebuilds every step. WGSL has no closures, so a shader writes the three loops itself:
// rows from `.z` to `.w` and columns from `.x` to `.y` of `walk_bounds`, each cell through `walk_cell`.

// Most agents one walk visits before it stops, the cap of a radius fold.
const MAX_VISITS: u32 = 65536u;

// Most cells one walk covers, empty cells included. A wider walk stops before its first cell.
const MAX_WALK_CELLS: u32 = 65536u;

// Number of cells a walk of `walk_bounds` covers, or `MAX_WALK_CELLS + 1` for any number past the cap. A negative
// radius covers none.
fn walk_cells(bounds: vec4<i32>) -> u32 {
    if (bounds.y < bounds.x || bounds.w < bounds.z) {
        return 0u;
    }
    let columns = u32(bounds.y - bounds.x + 1);
    let rows = u32(bounds.w - bounds.z + 1);
    // The product of two sides of a large index would wrap.
    if (columns > MAX_WALK_CELLS / rows) {
        return MAX_WALK_CELLS + 1u;
    }
    return columns * rows;
}

// Columns `.x` to `.y` and rows `.z` to `.w` a walk covers around `(x, y)`, inclusive and before wrapping.
//
// `grid` is the index's size in cells, `cell` the size of one cell and `cell_inv` its inverse, as the host computes
// it. A radius wider than the index walks each column or row once.
fn walk_bounds(x: f32, y: f32, radius: f32, grid: vec2<u32>, cell: vec2<f32>, cell_inv: vec2<f32>) -> vec4<i32> {
    let grid_w = i32(grid.x);
    let grid_h = i32(grid.y);
    let reach_x = i32(ceil(radius / cell.x));
    let reach_y = i32(ceil(radius / cell.y));
    let cell_x = wrap_index(i32(floor(x * cell_inv.x)), grid_w);
    let cell_y = wrap_index(i32(floor(y * cell_inv.y)), grid_h);
    // `2 * reach + 1 > side`, written so that no reach overflows it.
    var bounds = vec4<i32>(cell_x - reach_x, cell_x + reach_x, cell_y - reach_y, cell_y + reach_y);
    if (reach_x > (grid_w - 1) / 2) {
        bounds.x = 0;
        bounds.y = grid_w - 1;
    }
    if (reach_y > (grid_h - 1) / 2) {
        bounds.z = 0;
        bounds.w = grid_h - 1;
    }
    return bounds;
}

// Flat index of the cell at column `column` and row `row` of a walk, wrapped onto the index.
fn walk_cell(column: i32, row: i32, grid: vec2<u32>) -> u32 {
    return u32(wrap_index(row, i32(grid.y))) * grid.x + u32(wrap_index(column, i32(grid.x)));
}
