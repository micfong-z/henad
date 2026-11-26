//! Keys of the draws of a lowered sheet.
//!
//! A line or start that draws holds one key. Each draw it lowers to takes [`compose_key`] of that key and the low 32
//! bits of FNV-1a 64 over the draw's slot path, such as `starts_as.row`, so a draw keeps its numbers while other
//! slots change. The key is written on the draw itself, since the lowering writes no function, so no call composes a
//! draw's key a second time. A line or start with no key takes the low 32 bits of FNV-1a 64 over `<sheet id>/<owner
//! id>`, and the checker warns of it.

use std::collections::BTreeMap;

use henad_core::explore::fingerprint::fnv1a64;

use super::Problem;
use crate::normal::compose_key;
use crate::sheet::check::Place;

/// One draw of a lowered sheet: its final key, the words naming its owner and the owner's table.
#[derive(Debug, Clone)]
pub(super) struct Draw {
    /// Key written on the draw.
    pub key: u32,
    /// Owner in words as the card and the Sheet pane name it, such as `line 2` or `the start of Status`.
    pub owner: String,
    /// Table of the owner.
    pub place: Place,
}

/// Returns an error for each draw whose key an earlier draw of another owner takes too, naming both owners (V18).
///
/// Two draws of one owner, which share a table, never meet, since their slot paths differ.
pub(super) fn clashes(draws: &[Draw]) -> Vec<Problem> {
    let mut first: BTreeMap<u32, &Draw> = BTreeMap::new();
    let mut problems = Vec::new();
    for draw in draws {
        match first.get(&draw.key) {
            Some(other) if other.place != draw.place => problems.push(Problem {
                message: format!(
                    "{} draws with key {}, which {} also draws with. Draw new numbers on one of them",
                    draw.owner, draw.key, other.owner
                ),
                place: draw.place.clone(),
            }),
            Some(_) => {}
            None => {
                first.insert(draw.key, draw);
            }
        }
    }
    problems
}

/// Returns the key of a line or start with id `owner_id` in sheet `sheet_id`: `stored`, or the key derived from the two
/// ids when the sheet stores none.
pub(super) fn owner_key(stored: Option<u32>, sheet_id: &str, owner_id: &str) -> u32 {
    stored.unwrap_or_else(|| low_bits(&format!("{sheet_id}/{owner_id}")))
}

/// Returns the key of the draw at slot path `path` of an owner with key `key`.
pub(super) fn draw_key(key: u32, path: &str) -> u32 {
    compose_key(key, low_bits(path))
}

/// Returns the low 32 bits of FNV-1a 64 over `text`.
fn low_bits(text: &str) -> u32 {
    // The truncation keeps the low 32 bits, as the author format says.
    (fnv1a64(text.as_bytes()) & u64::from(u32::MAX)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_key_comes_from_the_sheet_and_owner_ids() {
        assert_eq!(owner_key(Some(7), "life", "l2"), 7);
        let derived = owner_key(None, "life", "l2");
        assert_eq!(
            derived,
            owner_key(None, "life", "l2"),
            "the derived key is deterministic"
        );
        assert_ne!(derived, owner_key(None, "life", "l3"));
        assert_ne!(derived, owner_key(None, "vote", "l2"));
        assert_eq!(u64::from(derived), fnv1a64(b"life/l2") & 0xffff_ffff);
    }

    #[test]
    fn each_slot_path_composes_its_own_key() {
        assert_ne!(draw_key(7, "starts_as.row"), draw_key(7, "starts_as.value"));
        assert_ne!(draw_key(7, "starts_as.row"), draw_key(8, "starts_as.row"));
        assert_eq!(draw_key(7, "starts_as.row"), compose_key(7, low_bits("starts_as.row")));
    }
}
