//! Bucket queue of live nodes, keyed by the tick at which each last joined a team.

/// Marks the end of a bucket's list.
const NIL: u32 = u32::MAX;

/// Returns the key a node is filed under, given the tick of its last team.
///
/// Setup nodes have `team_tick` 0, but NetLogo first counts their downtime on the first tick,
/// so they retire as if they had joined a team at tick 1.
pub(super) fn retirement_key(team_tick: u64) -> u64 {
    team_tick.max(1)
}

/// Returns the key of the nodes due to retire at the end of tick `now`, or `None` if no key is that old yet.
///
/// A node retires once it has been idle for more than `max_downtime` ticks.
pub(super) fn due_key(now: u64, max_downtime: u32) -> Option<u64> {
    now.checked_sub(u64::from(max_downtime) + 1)
}

/// Live nodes filed in buckets by the tick of their last team. The nodes due to retire are found without a scan.
///
/// The number of buckets is a power of two, at least the largest `max_downtime + 2` seen, and never shrinks.
/// Every live key lies after the last drained key and no later than the current tick.
/// That span is at most `max_downtime + 2` keys, or the previous value's span in the tick `max_downtime` is lowered.
/// Either way it fits in the buckets, and no two live keys share one.
/// Each bucket is a doubly linked list threaded through `prev` and `next`. A node moves between buckets in constant
/// time.
#[derive(Default)]
pub(super) struct RetirementRing {
    /// First node in each bucket, or `NIL` if the bucket is empty.
    heads: Vec<u32>,
    // Neighbours of each node in its bucket's list, or `NIL` at either end.
    prev: Vec<u32>,
    next: Vec<u32>,
    /// Key each node is filed under.
    key: Vec<u64>,
    /// Last key whose bucket has been drained. No live node has this key or an older one.
    drained: u64,
}

impl RetirementRing {
    /// Makes room for keys spanning `max_downtime + 2` ticks.
    ///
    /// If the buckets are too few, they are laid out again from `live`, at a cost of O(live).
    /// A lower `max_downtime` needs no layout. [`Self::drain_through`] then drains the keys it made overdue.
    pub fn prepare(&mut self, max_downtime: u32, live: &[u32], team_tick: &[u64]) {
        let needed = (max_downtime as usize + 2).next_power_of_two();
        if self.heads.len() >= needed {
            return;
        }
        self.heads.clear();
        self.heads.resize(needed, NIL);
        for &i in live {
            self.insert(i, retirement_key(team_tick[i as usize]));
        }
    }

    fn bucket(&self, key: u64) -> usize {
        (key & (self.heads.len() as u64 - 1)) as usize
    }

    /// Files `node` under `key`. The node must not be filed already.
    pub fn insert(&mut self, node: u32, key: u64) {
        let i = node as usize;
        if i >= self.key.len() {
            self.prev.resize(i + 1, NIL);
            self.next.resize(i + 1, NIL);
            self.key.resize(i + 1, 0);
        }
        let b = self.bucket(key);
        let head = self.heads[b];
        self.prev[i] = NIL;
        self.next[i] = head;
        if head != NIL {
            self.prev[head as usize] = node;
        }
        self.heads[b] = node;
        self.key[i] = key;
    }

    /// Takes `node` out of its bucket.
    pub fn remove(&mut self, node: u32) {
        let i = node as usize;
        let (prev, next) = (self.prev[i], self.next[i]);
        if prev == NIL {
            let b = self.bucket(self.key[i]);
            self.heads[b] = next;
        } else {
            self.next[prev as usize] = next;
        }
        if next != NIL {
            self.prev[next as usize] = prev;
        }
    }

    /// Empties into `out` the buckets of every key after the last drained one, up to and including `due`.
    ///
    /// This is one key per tick while `max_downtime` holds, more in the tick it is lowered,
    /// and none for a while after it is raised.
    pub fn drain_through(&mut self, due: u64, out: &mut Vec<u32>) {
        while self.drained < due {
            self.drained += 1;
            let b = self.bucket(self.drained);
            let mut node = std::mem::replace(&mut self.heads[b], NIL);
            while node != NIL {
                out.push(node);
                node = self.next[node as usize];
            }
        }
    }

    pub fn heap_bytes(&self) -> usize {
        (self.heads.capacity() + self.prev.capacity() + self.next.capacity()) * size_of::<u32>()
            + self.key.capacity() * size_of::<u64>()
    }
}

#[cfg(test)]
mod tests {
    use super::{RetirementRing, due_key, retirement_key};

    fn drained(ring: &mut RetirementRing, due: u64) -> Vec<u32> {
        let mut out = Vec::new();
        ring.drain_through(due, &mut out);
        out.sort_unstable();
        out
    }

    #[test]
    fn a_node_is_drained_from_the_bucket_it_was_last_filed_under() {
        let mut ring = RetirementRing::default();
        ring.prepare(3, &[0, 1, 2, 3], &[1, 1, 1, 1]);

        ring.remove(2);
        ring.insert(2, 3);
        ring.remove(0);
        ring.insert(0, 4);

        assert_eq!(drained(&mut ring, 1), [1, 3]);
        assert!(drained(&mut ring, 2).is_empty());
        assert_eq!(drained(&mut ring, 3), [2]);
        assert_eq!(drained(&mut ring, 4), [0]);
        assert!(drained(&mut ring, 4).is_empty(), "a key was drained twice");
    }

    #[test]
    fn removing_the_head_of_a_bucket_keeps_the_rest() {
        let mut ring = RetirementRing::default();
        ring.prepare(5, &[0, 1, 2], &[2, 2, 2]);
        // The last node inserted is the head.
        ring.remove(2);
        ring.remove(0);
        assert_eq!(drained(&mut ring, 2), [1]);
    }

    /// Lowering `max_downtime` moves the due key forward, and every key it passes is drained at once.
    #[test]
    fn a_lower_max_downtime_drains_every_key_it_makes_overdue() {
        let mut ring = RetirementRing::default();
        let team_tick = [2, 8, 10, 1];
        ring.prepare(20, &[0, 1, 2, 3], &team_tick);
        assert!(drained(&mut ring, due_key(10, 20).unwrap_or(0)).is_empty());

        ring.prepare(4, &[0, 1, 2, 3], &team_tick);
        let due = due_key(10, 4).expect("tick 10 is past the downtime");
        assert_eq!(due, 5);
        assert_eq!(
            drained(&mut ring, due),
            [0, 3],
            "the nodes idle since ticks 1 and 2 are overdue"
        );
        assert_eq!(drained(&mut ring, 8), [1]);
        assert_eq!(drained(&mut ring, 10), [2]);
    }

    /// Raising `max_downtime` past the bucket count lays the buckets out again, and every node keeps its key.
    #[test]
    fn a_higher_max_downtime_lays_the_buckets_out_again() {
        let mut ring = RetirementRing::default();
        let team_tick = [1, 3, 6];
        ring.prepare(2, &[0, 1, 2], &team_tick);
        ring.prepare(100, &[0, 1, 2], &team_tick);
        assert_eq!(drained(&mut ring, 1), [0]);
        assert_eq!(drained(&mut ring, 5), [1]);
        assert_eq!(drained(&mut ring, 6), [2]);
    }

    #[test]
    fn setup_nodes_retire_one_tick_later_than_their_team_tick_says() {
        assert_eq!(retirement_key(0), 1);
        assert_eq!(retirement_key(7), 7);
        assert_eq!(due_key(3, 3), None);
        assert_eq!(due_key(5, 3), Some(1));
        assert_eq!(due_key(9, u32::MAX), None);
    }
}
