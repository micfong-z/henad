#define_import_path shared::dims

// Grid size and the display texture size. The two differ once the grid outgrows the texture cap,
// so a display pass reads the cell at `texel * grid / tex`.
struct Dims {
    grid: vec2<u32>,
    tex: vec2<u32>,
}

// Cell a display texel samples. Identity while the grid still fits the texture.
fn cell_at(texel: vec2<u32>, dims: Dims) -> vec2<u32> {
    return texel * dims.grid / dims.tex;
}
