//! Snapshot undo of a graph: the canonical text of the whole file before each edit, `[editor]` included.
//!
//! An edit needs no inverse. Undo reads the snapshot back, and the canvas follows the document it reads. A *coalesce
//! key* names a gesture that spans many edits, such as a value dragged in a field. Consecutive edits with one key make
//! one snapshot.

/// Most snapshots each stack holds. The oldest is dropped past it.
pub const MAX_SNAPSHOTS: usize = 200;

/// Undo and redo stacks of canonical texts.
#[derive(Debug, Default)]
pub struct History {
    /// Texts before each edit, oldest first.
    undo: Vec<String>,
    /// Texts an undo replaced, the most recent last.
    redo: Vec<String>,
    /// Coalesce key of the last edit recorded.
    last_key: Option<String>,
}

impl History {
    /// Records `before`, the text before an edit, unless the last edit carried the same coalesce key `key`.
    ///
    /// Any edit clears the redo stack.
    pub fn record(&mut self, before: String, key: Option<&str>) {
        self.redo.clear();
        let coalesced = key.is_some() && self.last_key.as_deref() == key;
        self.last_key = key.map(str::to_owned);
        if coalesced {
            return;
        }
        self.undo.push(before);
        if self.undo.len() > MAX_SNAPSHOTS {
            self.undo.remove(0);
        }
    }

    /// Returns the text to restore for an undo of `current`, or `None` when there is nothing to undo.
    pub fn undo(&mut self, current: String) -> Option<String> {
        let restored = self.undo.pop()?;
        self.redo.push(current);
        self.last_key = None;
        Some(restored)
    }

    /// Returns the text to restore for a redo of `current`, or `None` when there is nothing to redo.
    pub fn redo(&mut self, current: String) -> Option<String> {
        let restored = self.redo.pop()?;
        self.undo.push(current);
        self.last_key = None;
        Some(restored)
    }

    /// Returns whether an undo has a text to restore.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Number of edits an undo can step back over.
    #[cfg(test)]
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// Returns whether a redo has a text to restore.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Ends the gesture of the last edit, so the next edit makes a snapshot whatever its key.
    pub fn end_gesture(&mut self) {
        self.last_key = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{History, MAX_SNAPSHOTS};

    #[test]
    fn undo_and_redo_walk_the_snapshots_and_an_edit_clears_redo() {
        let mut history = History::default();
        history.record("a".to_owned(), None);
        history.record("b".to_owned(), None);
        assert_eq!(history.undo("c".to_owned()).as_deref(), Some("b"));
        assert_eq!(history.undo("b".to_owned()).as_deref(), Some("a"));
        assert_eq!(history.undo("a".to_owned()), None);
        assert_eq!(history.redo("a".to_owned()).as_deref(), Some("b"));
        history.record("b".to_owned(), None);
        assert!(!history.can_redo(), "an edit clears redo");
        assert!(history.can_undo());
    }

    #[test]
    fn edits_under_one_key_make_one_snapshot() {
        let mut history = History::default();
        history.record("a".to_owned(), Some("drag"));
        history.record("b".to_owned(), Some("drag"));
        history.record("c".to_owned(), Some("drag"));
        assert_eq!(history.undo("d".to_owned()).as_deref(), Some("a"));
        assert!(!history.can_undo());

        history.record("a".to_owned(), Some("drag"));
        history.end_gesture();
        history.record("b".to_owned(), Some("drag"));
        assert_eq!(history.undo("c".to_owned()).as_deref(), Some("b"));
    }

    #[test]
    fn the_oldest_snapshot_goes_past_the_limit() {
        let mut history = History::default();
        for index in 0..=MAX_SNAPSHOTS {
            history.record(index.to_string(), None);
        }
        let mut oldest = None;
        let mut current = String::new();
        while let Some(text) = history.undo(current.clone()) {
            current.clone_from(&text);
            oldest = Some(text);
        }
        assert_eq!(oldest.as_deref(), Some("1"));
    }
}
