//! A dynamic graph represented using CSR-like rows of neighbours, with an edge list.
//!
//! "Rows" are the adjacency lists of each node (i.e. a row in the adjacency matrix).
//! All rows are stored in compressed sparse row (CSR) format.

use std::ops::Range;

/// Rows stored in compressed sparse row (CSR) format with slack specified by [`Csr::MIN_ROW`] and [`Csr::REBUILD_SLACK`].
///
/// When a row is full, [`Csr::relocate`] will be called.
#[derive(Default)]
struct Csr {
    /// Start index of each row. Note that `offset[i+1]-offset[i]` is not a reliable way of finding the length of a row,
    /// due to relocations and slack. Use `len[i]` instead.
    offset: Vec<u32>,
    /// Length (without slack) of each row.
    len: Vec<u32>,
    /// Capacity (with slack) of each row.
    capacity: Vec<u32>,
    /// Neighbour node of each entry.
    neighbors: Vec<u32>,
    /// Edge index of each entry. This is used in `Network` to find the corresponding edge in the edge list.
    edges: Vec<u32>,
    /// Number of stale entries caused by relocations.
    stale_count: usize,
}

impl Csr {
    /// Minimum row size to avoid frequent rebuilding on small graphs.
    const MIN_ROW: usize = 4;

    /// Fractional slack (1/[`Self::REBUILD_SLACK`]) to leave per row when rebuilding.
    const REBUILD_SLACK: usize = 4;

    /// Initialises a CSR with `n` empty rows.
    fn with_rows(n: usize) -> Self {
        Self {
            offset: vec![0; n],
            len: vec![0; n],
            capacity: vec![0; n],
            ..Self::default()
        }
    }

    /// Returns the range of indices in `neighbors` and `edges` that correspond to row `i`.
    fn span(&self, i: u32) -> Range<usize> {
        let start = self.offset[i as usize] as usize;
        start..start + self.len[i as usize] as usize
    }

    /// Returns the slice of neighbour nodes for row `i`.
    fn row(&self, i: u32) -> &[u32] {
        &self.neighbors[self.span(i)]
    }

    /// Returns the slice of edge indices for row `i`.
    fn row_edges(&self, i: u32) -> &[u32] {
        &self.edges[self.span(i)]
    }

    /// Adds a new empty row at the end of the CSR.
    ///
    /// Note that this does not allocate space for the row.
    /// Upon the first push, it will be relocated to a new space with capacity [`Self::MIN_ROW`].
    fn push_row(&mut self) {
        self.offset.push(0);
        self.len.push(0);
        self.capacity.push(0);
    }

    /// Moves node `i`'s row to the end with double capacity, leaving the old space as stale.
    fn relocate(&mut self, i: u32) {
        let span = self.span(i);
        let i = i as usize;
        let new_capacity = (self.capacity[i] as usize * 2).max(Self::MIN_ROW);
        let new_offset = self.neighbors.len();

        self.neighbors.extend_from_within(span.clone());
        self.edges.extend_from_within(span);
        self.neighbors.resize(new_offset + new_capacity, 0);
        self.edges.resize(new_offset + new_capacity, 0);

        self.stale_count += self.capacity[i] as usize;
        self.offset[i] = new_offset as u32;
        self.capacity[i] = new_capacity as u32;
    }

    /// Appends `neighbor`, `edge` to node `i`'s rows.
    fn push(&mut self, i: u32, neighbor: u32, edge: u32) {
        if self.len[i as usize] == self.capacity[i as usize] {
            self.relocate(i);
        }
        let offset = self.offset[i as usize] as usize + self.len[i as usize] as usize;
        self.neighbors[offset] = neighbor;
        self.edges[offset] = edge;
        self.len[i as usize] += 1;
    }

    /// Drops the entry for `edge` from node `i`'s row.
    ///
    /// Returns whether the edge was found and successfully removed.
    fn remove(&mut self, i: u32, edge: u32) -> bool {
        let Range { start, end } = self.span(i);
        let Some(offset) = self.edges[start..end].iter().position(|&e| e == edge) else {
            return false;
        };
        let offset = start + offset;
        self.neighbors[offset] = self.neighbors[end - 1];
        self.edges[offset] = self.edges[end - 1];
        self.len[i as usize] -= 1;
        true
    }

    /// Replaces `old` with `new` in the edge list of node `i`.
    fn renumber_edge(&mut self, i: u32, old: u32, new: u32) {
        let span = self.span(i);
        if let Some(slot) = self.edges[span].iter_mut().find(|e| **e == old) {
            *slot = new;
        }
    }

    /// Removes all entries from node `i`'s row, leaving the space as stale.
    fn clear_row(&mut self, i: u32) {
        self.stale_count += self.capacity[i as usize] as usize;
        self.offset[i as usize] = 0;
        self.len[i as usize] = 0;
        self.capacity[i as usize] = 0;
    }

    /// Packs every row from scratch, dropping every stale entry.
    ///
    /// `entries` is an iterator over `(row, neighbor, edge)` tuples that describe every entry in the graph.
    fn build(&mut self, n: usize, entries: impl Iterator<Item = (u32, u32, u32)> + Clone) {
        self.offset.clear();
        self.offset.resize(n, 0);
        self.len.clear();
        self.len.resize(n, 0);
        self.capacity.clear();
        self.capacity.resize(n, 0);

        for (row, _, _) in entries.clone() {
            self.len[row as usize] += 1;
        }

        let mut total = 0;
        for i in 0..n {
            let new_offset = total;
            let new_capacity = (self.len[i] as usize + self.len[i] as usize / Self::REBUILD_SLACK).max(Self::MIN_ROW);
            self.capacity[i] = new_capacity as u32;
            self.offset[i] = new_offset as u32;
            total += new_capacity;
        }

        self.len.fill(0);
        self.neighbors.clear();
        self.neighbors.resize(total, 0);
        self.edges.clear();
        self.edges.resize(total, 0);
        self.stale_count = 0;

        for (row, neighbor, edge) in entries {
            let i = row as usize;
            let offset = self.offset[i] as usize + self.len[i] as usize;
            self.neighbors[offset] = neighbor;
            self.edges[offset] = edge;
            self.len[i] += 1;
        }
    }

    /// Sets the CSR empty.
    fn clear(&mut self) {
        self.offset.clear();
        self.len.clear();
        self.capacity.clear();
        self.neighbors.clear();
        self.edges.clear();
        self.stale_count = 0;
    }

    fn heap_bytes(&self) -> usize {
        (self.offset.capacity()
            + self.len.capacity()
            + self.capacity.capacity()
            + self.neighbors.capacity()
            + self.edges.capacity())
            * size_of::<u32>()
    }
}

/// A network of nodes and edges.
///
/// Rows are stored in compressed sparse row (CSR) format, with an edge list for the view to draw.
pub struct Network {
    occupied: Vec<bool>,
    /// Reusable indices of retired nodes
    free: Vec<u32>,
    node_count: usize,

    src: Vec<u32>,
    dst: Vec<u32>,
    color: Vec<u8>,

    /// By target. For undirected graphs, this stores both directions.
    in_csr: Csr,
    /// By source. For undirected graphs, rows are stored in [`Self::in_csr`] instead.
    out_csr: Csr,

    directed: bool,

    /// Used to detect when a view of the graph is out of date.
    version: u64,
}

impl Network {
    /// Creates a new `Network` with `nodes` slots and no edges. All slots are considered occupied with a node.
    pub fn new(nodes: usize, directed: bool) -> Self {
        Self {
            occupied: vec![true; nodes],
            free: Vec::new(),
            node_count: nodes,
            src: Vec::new(),
            dst: Vec::new(),
            color: Vec::new(),
            in_csr: Csr::with_rows(nodes),
            out_csr: Csr::with_rows(if directed { nodes } else { 0 }),
            directed,
            version: 0,
        }
    }

    /// Number of total slots.
    pub fn slot_count(&self) -> usize {
        self.occupied.len()
    }

    /// Number of nodes.
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// Whether slot `i` is occupied by a node.
    pub fn contains_node(&self, i: u32) -> bool {
        self.occupied.get(i as usize).copied().unwrap_or(false)
    }

    pub fn directed(&self) -> bool {
        self.directed
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    /// Spawns a new node and returns its index.
    pub fn spawn(&mut self) -> u32 {
        self.node_count += 1;
        if let Some(i) = self.free.pop() {
            self.occupied[i as usize] = true;
            return i;
        }
        let i = self.occupied.len() as u32;
        self.occupied.push(true);
        self.in_csr.push_row();
        if self.directed {
            self.out_csr.push_row();
        }
        i
    }

    /// Retires node in slot `i`.
    pub fn retire(&mut self, i: u32) {
        if !self.contains_node(i) {
            return;
        }

        // Collected before anything moves, then removed high index first so a swap never lands on
        // one still to come.
        let mut doomed: Vec<u32> = self.in_csr.row_edges(i).to_vec();
        if self.directed {
            doomed.extend_from_slice(self.out_csr.row_edges(i));
        }
        doomed.sort_unstable();
        doomed.dedup();
        for &e in doomed.iter().rev() {
            self.remove_edge(e);
        }

        self.in_csr.clear_row(i);
        if self.directed {
            self.out_csr.clear_row(i);
        }
        self.occupied[i as usize] = false;
        self.node_count -= 1;
        self.free.push(i);
        self.version += 1;
    }

    pub fn edge_count(&self) -> usize {
        self.src.len()
    }

    /// Returns the edge list as `(src, dst, color)`.
    pub fn edges(&self) -> (&[u32], &[u32], &[u8]) {
        (&self.src, &self.dst, &self.color)
    }

    /// Returns a mutable slice of the edge colors.
    ///
    /// The version is incremented to indicate that the view should be updated.
    pub fn colors_mut(&mut self) -> &mut [u8] {
        self.version += 1;
        &mut self.color
    }

    /// Sets the color of edge `edge` to `color`.
    pub fn set_edge_color(&mut self, edge: u32, color: u8) {
        self.color[edge as usize] = color;
        self.version += 1;
    }

    /// Creates a new edge from `a` to `b` with color `color`, returning its index in the edge list.
    ///
    /// Repeated edges are allowed.
    ///
    /// # Panics
    /// Panics on debug builds if `a == b`.
    pub fn add_edge(&mut self, a: u32, b: u32, color: u8) -> u32 {
        debug_assert!(a != b, "a self loop has no meaning for either row set");
        let e = self.src.len() as u32;
        self.src.push(a);
        self.dst.push(b);
        self.color.push(color);
        if self.directed {
            self.in_csr.push(b, a, e);
            self.out_csr.push(a, b, e);
        } else {
            self.in_csr.push(b, a, e);
            self.in_csr.push(a, b, e);
        }
        self.version += 1;
        e
    }

    /// Removes edge `edge` from the graph.
    pub fn remove_edge(&mut self, edge: u32) {
        let e = edge as usize;
        let (a, b) = (self.src[e], self.dst[e]);
        if self.directed {
            self.in_csr.remove(b, edge);
            self.out_csr.remove(a, edge);
        } else {
            self.in_csr.remove(b, edge);
            self.in_csr.remove(a, edge);
        }

        self.src.swap_remove(e);
        self.dst.swap_remove(e);
        self.color.swap_remove(e);

        // The edge that moved into this slot is still in its endpoints' rows under its old index.
        let moved = self.src.len() as u32;
        if e < self.src.len() {
            let (ma, mb) = (self.src[e], self.dst[e]);
            if self.directed {
                self.in_csr.renumber_edge(mb, moved, edge);
                self.out_csr.renumber_edge(ma, moved, edge);
            } else {
                self.in_csr.renumber_edge(mb, moved, edge);
                self.in_csr.renumber_edge(ma, moved, edge);
            }
        }
        self.version += 1;
    }

    /// Returns the nodes that have an edge to `i`.
    pub fn in_neighbors(&self, i: u32) -> &[u32] {
        self.in_csr.row(i)
    }

    /// Returns the nodes that `i` has an edge to.
    pub fn out_neighbors(&self, i: u32) -> &[u32] {
        if self.directed {
            self.out_csr.row(i)
        } else {
            self.in_csr.row(i)
        }
    }

    /// Returns the degree of the node. On directed graphs, this is the sum of in-degree and out-degree.
    pub fn degree(&self, i: u32) -> usize {
        if self.directed {
            self.in_csr.row(i).len() + self.out_csr.row(i).len()
        } else {
            self.in_csr.row(i).len()
        }
    }

    /// Returns whether there is an edge from `a` to `b`.
    pub fn has_edge(&self, a: u32, b: u32) -> bool {
        if self.directed {
            self.out_csr.row(a).contains(&b)
        } else {
            let (a, b) = if self.degree(a) <= self.degree(b) {
                (a, b)
            } else {
                (b, a)
            };
            self.in_csr.row(a).contains(&b)
        }
    }

    pub fn set_directed(&mut self, directed: bool) {
        if directed == self.directed {
            return;
        }
        self.directed = directed;
        self.rebuild();
        self.version += 1;
    }

    /// Returns whether the graph should be repacked to reclaim space from relocated rows.
    ///
    /// This is true when the number of stale entries exceeds 1/2 of occupied rows and is greater than [`Csr::MIN_ROW`].
    pub fn should_repack(&self) -> bool {
        let stale = self.in_csr.stale_count + self.out_csr.stale_count;
        let slots = self.in_csr.neighbors.len() + self.out_csr.neighbors.len();
        stale > slots / 2 && stale > Csr::MIN_ROW
    }

    /// Rebuilds the CSR rows from the edge list, dropping any stale entries.
    pub fn rebuild(&mut self) {
        let n = self.occupied.len();
        let (src, dst) = (&self.src, &self.dst);
        // Each edge as `(src, dst, index)`, already the shape of an out-row entry.
        let edges = || src.iter().zip(dst).enumerate().map(|(e, (&a, &b))| (a, b, e as u32));
        if self.directed {
            self.in_csr.build(n, edges().map(|(a, b, e)| (b, a, e)));
            self.out_csr.build(n, edges());
        } else {
            self.in_csr
                .build(n, edges().flat_map(|(a, b, e)| [(b, a, e), (a, b, e)]));
            self.out_csr.clear();
        }
    }

    pub fn heap_bytes(&self) -> usize {
        self.occupied.capacity()
            + self.free.capacity() * size_of::<u32>()
            + (self.src.capacity() + self.dst.capacity()) * size_of::<u32>()
            + self.color.capacity()
            + self.in_csr.heap_bytes()
            + self.out_csr.heap_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::Network;
    use crate::authoring::primitives::rng::{next_bits, xorshift64};

    /// Neighbours of every node, computed from the edge list alone.
    fn brute_force(net: &Network) -> (Vec<Vec<u32>>, Vec<Vec<u32>>) {
        let n = net.slot_count();
        let (src, dst, _) = net.edges();
        let mut ins = vec![Vec::new(); n];
        let mut outs = vec![Vec::new(); n];
        for e in 0..src.len() {
            let (a, b) = (src[e], dst[e]);
            ins[b as usize].push(a);
            if net.directed() {
                outs[a as usize].push(b);
            } else {
                ins[a as usize].push(b);
            }
        }
        if !net.directed() {
            outs = ins.clone();
        }
        (ins, outs)
    }

    fn sorted(mut v: Vec<u32>) -> Vec<u32> {
        v.sort_unstable();
        v
    }

    /// Asserts that every node's rows match the edge list.
    fn assert_rows_match(net: &Network, what: &str) {
        let (ins, outs) = brute_force(net);
        for i in 0..net.slot_count() as u32 {
            assert_eq!(
                sorted(net.in_neighbors(i).to_vec()),
                sorted(ins[i as usize].clone()),
                "{what}: node {i} in-row disagrees with the edge list"
            );
            assert_eq!(
                sorted(net.out_neighbors(i).to_vec()),
                sorted(outs[i as usize].clone()),
                "{what}: node {i} out-row disagrees with the edge list"
            );
        }
    }

    #[test]
    fn an_undirected_edge_lands_in_both_rows() {
        let mut net = Network::new(4, false);
        net.add_edge(0, 3, 0);
        assert_eq!(net.in_neighbors(0), &[3]);
        assert_eq!(net.in_neighbors(3), &[0]);
        assert_eq!(net.out_neighbors(0), net.in_neighbors(0), "one row answers both ways");
        assert!(net.has_edge(0, 3) && net.has_edge(3, 0));
        assert_eq!(net.degree(0), 1);
    }

    #[test]
    fn a_directed_edge_lands_in_one_row_each_way() {
        let mut net = Network::new(4, true);
        net.add_edge(0, 3, 0);
        assert_eq!(net.out_neighbors(0), &[3]);
        assert!(net.in_neighbors(0).is_empty(), "0 has no edge into it");
        assert_eq!(net.in_neighbors(3), &[0]);
        assert!(net.out_neighbors(3).is_empty());
        assert!(net.has_edge(0, 3), "the edge runs 0 to 3");
        assert!(!net.has_edge(3, 0), "and not the other way");
        assert_eq!(net.degree(0), 1);
    }

    #[test]
    fn a_row_survives_the_relocation_its_growth_forces() {
        let mut net = Network::new(40, false);
        // Well past `Csr::MIN_ROW`, forcing several relocations.
        for b in 1..40u32 {
            net.add_edge(0, b, 0);
        }
        assert_eq!(sorted(net.in_neighbors(0).to_vec()), (1..40).collect::<Vec<_>>());
        assert_rows_match(&net, "after growth");
    }

    #[test]
    fn removing_an_edge_renumbers_the_one_that_takes_its_place() {
        let mut net = Network::new(6, false);
        for (a, b) in [(0, 1), (2, 3), (4, 5)] {
            net.add_edge(a, b, 0);
        }
        // Edge 2 moves into index 0, and its rows must follow.
        net.remove_edge(0);
        assert_eq!(net.edge_count(), 2);
        assert_rows_match(&net, "after a middle removal");
        net.remove_edge(0);
        assert_rows_match(&net, "after a second removal");
        assert_eq!(net.edge_count(), 1);
    }

    #[test]
    fn retiring_a_node_takes_every_edge_touching_it() {
        let mut net = Network::new(6, false);
        for b in [1, 2, 3, 4] {
            net.add_edge(0, b, 0);
        }
        net.add_edge(1, 2, 0);
        net.retire(0);

        assert!(!net.contains_node(0));
        assert_eq!(net.node_count(), 5);
        assert_eq!(net.edge_count(), 1, "only the edge clear of node 0 is left");
        assert_rows_match(&net, "after a retirement");
        for i in 1..6u32 {
            assert!(
                !net.in_neighbors(i).contains(&0),
                "node {i} still lists the retired node"
            );
        }
    }

    #[test]
    fn retiring_a_node_takes_its_edges_when_directed_too() {
        let mut net = Network::new(6, true);
        net.add_edge(0, 1, 0);
        net.add_edge(2, 0, 0);
        net.add_edge(3, 4, 0);
        net.retire(0);
        assert_eq!(net.edge_count(), 1);
        assert_rows_match(&net, "after a directed retirement");
    }

    #[test]
    fn a_retired_slot_is_the_next_one_handed_out() {
        let mut net = Network::new(3, false);
        net.retire(1);
        net.retire(2);
        assert_eq!(net.spawn(), 2, "newest free slot first");
        assert_eq!(net.spawn(), 1);
        assert_eq!(net.spawn(), 3, "then a fresh one");
        assert_eq!(net.slot_count(), 4);
        assert_eq!(net.node_count(), 4);
    }

    #[test]
    fn a_reused_slot_starts_with_no_edges() {
        let mut net = Network::new(4, false);
        net.add_edge(1, 2, 0);
        net.add_edge(1, 3, 0);
        net.retire(1);
        let reused = net.spawn();
        assert_eq!(reused, 1);
        assert!(
            net.in_neighbors(reused).is_empty(),
            "the old row came back with the slot"
        );
        net.add_edge(reused, 2, 0);
        assert_rows_match(&net, "after reuse");
    }

    #[test]
    fn a_rebuild_leaves_the_same_graph() {
        let mut net = Network::new(20, false);
        let mut rng = 0x51A7_u64;
        for _ in 0..60 {
            let a = next_bits(&mut rng) % 20;
            let b = next_bits(&mut rng) % 20;
            if a != b && !net.has_edge(a, b) {
                net.add_edge(a, b, 0);
            }
        }
        let before: Vec<Vec<u32>> = (0..20).map(|i| sorted(net.in_neighbors(i).to_vec())).collect();
        net.rebuild();
        let after: Vec<Vec<u32>> = (0..20).map(|i| sorted(net.in_neighbors(i).to_vec())).collect();
        assert_eq!(before, after, "a repack changed the neighbours");
        assert_rows_match(&net, "after a rebuild");
    }

    #[test]
    fn flipping_the_direction_rebuilds_both_row_sets() {
        let mut net = Network::new(5, false);
        net.add_edge(0, 1, 0);
        net.add_edge(1, 2, 0);
        net.set_directed(true);

        assert!(net.directed());
        assert_eq!(net.out_neighbors(0), &[1]);
        assert!(net.in_neighbors(0).is_empty(), "0 has no edge into it once directed");
        assert_rows_match(&net, "after a flip to directed");

        net.set_directed(false);
        assert_rows_match(&net, "and back");
        assert_eq!(sorted(net.in_neighbors(1).to_vec()), vec![0, 2]);
    }

    /// Rows still match the edge list after a long run of random changes.
    #[test]
    fn rows_track_the_edge_list_through_random_churn() {
        for &directed in &[false, true] {
            let mut net = Network::new(30, directed);
            let mut rng = xorshift64(0xC0FF_EE01 ^ u64::from(directed));
            for round in 0..400 {
                match next_bits(&mut rng) % 10 {
                    0..=5 => {
                        let a = next_bits(&mut rng) % 30;
                        let b = next_bits(&mut rng) % 30;
                        if a != b && net.contains_node(a) && net.contains_node(b) && !net.has_edge(a, b) {
                            net.add_edge(a, b, 0);
                        }
                    }
                    6..=7 if net.edge_count() > 0 => {
                        let e = next_bits(&mut rng) % net.edge_count() as u32;
                        net.remove_edge(e);
                    }
                    8 => {
                        let i = next_bits(&mut rng) % net.slot_count() as u32;
                        net.retire(i);
                    }
                    _ => {
                        net.spawn();
                    }
                }
                assert_rows_match(&net, &format!("directed={directed} round={round}"));
            }
            assert!(
                net.edge_count() > 0,
                "the churn removed everything, so it proved little"
            );
        }
    }

    #[test]
    fn has_edge_agrees_with_the_edge_list() {
        let mut net = Network::new(12, false);
        let mut rng = 0xBEEF_u64;
        for _ in 0..30 {
            let a = next_bits(&mut rng) % 12;
            let b = next_bits(&mut rng) % 12;
            if a != b && !net.has_edge(a, b) {
                net.add_edge(a, b, 0);
            }
        }
        let (src, dst, _) = net.edges();
        let joined: Vec<(u32, u32)> = src.iter().zip(dst).map(|(&a, &b)| (a, b)).collect();
        for a in 0..12u32 {
            for b in 0..12u32 {
                let listed = joined.contains(&(a, b)) || joined.contains(&(b, a));
                assert_eq!(net.has_edge(a, b), a != b && listed, "has_edge({a}, {b})");
            }
        }
    }

    #[test]
    fn the_version_moves_for_every_change_the_view_can_see() {
        let mut net = Network::new(4, false);
        let start = net.version();
        let e = net.add_edge(0, 1, 0);
        assert!(net.version() > start, "an edge appeared");

        let after_add = net.version();
        net.set_edge_color(e, 3);
        assert!(net.version() > after_add, "a colour changed");

        let after_color = net.version();
        net.remove_edge(e);
        assert!(net.version() > after_color, "an edge went");

        let after_remove = net.version();
        net.set_directed(true);
        assert!(net.version() > after_remove, "the direction changed");
    }

    /// Growth alone never asks for a repack. Retiring most nodes does.
    #[test]
    fn retiring_most_of_the_graph_asks_for_a_repack() {
        let mut net = Network::new(40, false);
        let mut rng = 0x9A5B_u64;
        for _ in 0..250 {
            let a = next_bits(&mut rng) % 40;
            let b = next_bits(&mut rng) % 40;
            if a != b && !net.has_edge(a, b) {
                net.add_edge(a, b, 0);
            }
        }
        assert!(!net.should_repack(), "growth alone should not ask for one");

        for i in 0..38u32 {
            net.retire(i);
        }
        assert!(net.should_repack(), "38 cleared rows left nothing to reclaim");

        net.rebuild();
        assert!(!net.should_repack(), "the repack did not reclaim it");
        assert_rows_match(&net, "after the repack");
    }
}
