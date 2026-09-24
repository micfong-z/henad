// Moves one random edge to a random pair of nodes that are not already joined, mirroring
// `virus_network::wiring::rewire`. One invocation, since each try depends on the one before.
//
// The rows are left stale, and the caller rebuilds them before anything reads them again.

#import shared::rng::next_index
#import shared::graph::Edge

struct RewireParams {
    num_nodes: u32,
    num_edges: u32,
    directed: u32,
    // Number of draws before giving up, `wiring::REWIRE_TRIES`.
    tries: u32,
    // Index into `streams` of the word this pipeline draws from.
    stream: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<storage, read_write> edges: array<Edge>;
@group(0) @binding(1) var<storage, read>       row_start: array<u32>;
@group(0) @binding(2) var<storage, read>       entries: array<u32>;
@group(0) @binding(3) var<storage, read_write> streams: array<u32>;
@group(0) @binding(4) var<uniform>             params: RewireParams;

// Returns whether the in-row of `node` lists `neighbor`.
fn in_row_holds(node: u32, neighbor: u32) -> bool {
    let end = row_start[2u * node + 1u];
    for (var k = row_start[2u * node]; k < end; k++) {
        if (entries[k] == neighbor) {
            return true;
        }
    }
    return false;
}

// Returns whether an edge joins `a` and `b` in either direction. A directed in-row lists only sources, so both ends
// are asked.
fn joined(a: u32, b: u32) -> bool {
    return in_row_holds(a, b) || (params.directed == 1u && in_row_holds(b, a));
}

@compute
@workgroup_size(1)
fn main() {
    let n = params.num_nodes;
    let m = params.num_edges;
    if (n < 2u || m == 0u) {
        return;
    }

    var r = streams[params.stream];
    for (var t = 0u; t < params.tries; t++) {
        let a = next_index(&r, n);
        var b = next_index(&r, n - 1u);
        if (b >= a) {
            b += 1u;
        }
        if (!joined(a, b)) {
            // The edge list order of `Network::remove_edge` followed by `Network::add_edge`. The moved slot keeps its
            // colour until the next recolour.
            let e = next_index(&r, m);
            edges[e] = edges[m - 1u];
            edges[m - 1u].src = a;
            edges[m - 1u].dst = b;
            break;
        }
    }
    streams[params.stream] = r;
}
