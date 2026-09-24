#define_import_path shared::graph

// One edge of a network. `color` is packed RGBA, so the edge list can be drawn as an instance buffer.
struct Edge {
    src: u32,
    dst: u32,
    color: u32,
}
