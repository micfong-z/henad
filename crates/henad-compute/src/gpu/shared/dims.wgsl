#define_import_path shared::dims

// Grid size and the display texture size. The two differ once the grid outgrows the texture cap,
// so a display pass reads the cell at `texel * grid / tex`.
struct Dims {
    grid: vec2<u32>,
    tex: vec2<u32>,
}
