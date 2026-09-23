//! Dense list of the live nodes, for uniform draws that cost nothing per retired slot.

use henad_core::network::Network;

/// Marks a slot that is not in the list.
const ABSENT: u32 = u32::MAX;

/// Live nodes in a dense list, with each slot's position in it.
///
/// Slots are never compacted, so after a large cohort retires most of them are empty.
/// Drawing from this list instead of from the slots keeps a draw independent of how many slots there are.
#[derive(Default)]
pub(super) struct LiveSet {
    list: Vec<u32>,
    /// Position of each slot's node in `list`, or `ABSENT` if the slot is empty.
    pos: Vec<u32>,
}

impl LiveSet {
    pub fn as_slice(&self) -> &[u32] {
        &self.list
    }

    /// Lists every node in `graph` again, dropping whatever was listed before.
    pub fn rebuild(&mut self, graph: &Network) {
        self.list.clear();
        self.pos.clear();
        self.pos.resize(graph.slot_count(), ABSENT);
        for i in 0..graph.slot_count() as u32 {
            if graph.contains_node(i) {
                self.insert(i);
            }
        }
    }

    /// Adds `node`, which must not be in the list already.
    pub fn insert(&mut self, node: u32) {
        let i = node as usize;
        if i >= self.pos.len() {
            self.pos.resize(i + 1, ABSENT);
        }
        debug_assert_eq!(self.pos[i], ABSENT, "node {node} is already listed");
        self.pos[i] = self.list.len() as u32;
        self.list.push(node);
    }

    /// Removes `node` by moving the last entry into its place.
    pub fn remove(&mut self, node: u32) {
        let at = std::mem::replace(&mut self.pos[node as usize], ABSENT);
        debug_assert_ne!(at, ABSENT, "node {node} is not listed");
        let last = self.list.pop().expect("a listed node leaves the list non-empty");
        if last != node {
            self.list[at as usize] = last;
            self.pos[last as usize] = at;
        }
    }

    pub fn heap_bytes(&self) -> usize {
        (self.list.capacity() + self.pos.capacity()) * size_of::<u32>()
    }
}

#[cfg(test)]
mod tests {
    use super::LiveSet;

    fn sorted(set: &LiveSet) -> Vec<u32> {
        let mut v = set.as_slice().to_vec();
        v.sort_unstable();
        v
    }

    #[test]
    fn removal_keeps_every_other_node_listed() {
        let mut set = LiveSet::default();
        for i in [4, 0, 7, 2, 9] {
            set.insert(i);
        }
        set.remove(0);
        set.remove(9);
        assert_eq!(sorted(&set), [2, 4, 7]);
        set.insert(0);
        set.remove(4);
        assert_eq!(sorted(&set), [0, 2, 7]);
        for i in [0, 2, 7] {
            set.remove(i);
        }
        assert!(set.as_slice().is_empty());
    }
}
