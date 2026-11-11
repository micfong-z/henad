//! Namespaces of sheet ids and the minting of a new item's id from its name.

use super::SheetEditError;
use super::references::is_line_id;
use crate::sheet::check::{RESERVED_IDS, RESERVED_PARAMETER_IDS, RESERVED_PREFIX};
use crate::sheet::limits::MAX_SHEET_ID_BYTES;
use crate::sheet::sentence::ANY_STATE_ID;

/// A namespace of sheet ids, with the ids reserved in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IdSpace {
    /// Properties, parameters, measures and events, which the lowered model holds side by side.
    Item,
    /// Kinds of thing.
    Thing,
    /// Buttons.
    Button,
    /// Cards.
    Card,
    /// The states of one property.
    State,
}

impl IdSpace {
    /// Returns whether `id` is reserved in the namespace.
    pub fn is_reserved(self, id: &str) -> bool {
        if self == Self::State {
            return id == ANY_STATE_ID;
        }
        RESERVED_IDS.contains(&id)
            || is_line_id(id)
            || self == Self::Item && (RESERVED_PARAMETER_IDS.contains(&id) || id.starts_with(RESERVED_PREFIX))
            || self == Self::Thing && matches!(id, "cells" | "ground")
    }
}

/// Returns whether a new item named `name` in `space` would take a reserved id, as "Count" and "Grid width" would in
/// [`IdSpace::Item`].
///
/// ```
/// use henad_graph::sheet::edit::{IdSpace, is_reserved_name};
///
/// assert!(is_reserved_name("Count", IdSpace::Item));
/// assert!(is_reserved_name("Grid width", IdSpace::Item));
/// assert!(!is_reserved_name("Infection rate", IdSpace::Item));
/// ```
pub fn is_reserved_name(name: &str, space: IdSpace) -> bool {
    matches!(mint_id(name, space, |_| false), Err(SheetEditError::Reserved(_)))
}

/// Returns the id minted from `name` in `space`, where `is_taken` returns whether an id is already held.
///
/// The id is `name` in lower case with each run of other characters than `a-z` and `0-9` as one `_`, an `x` before a
/// leading digit, and at most [`MAX_SHEET_ID_BYTES`] bytes. A name with no letter or digit mints `x`. Against a clash
/// it takes `_2`, `_3` and so on.
///
/// ```
/// use henad_graph::sheet::edit::{IdSpace, mint_id};
///
/// assert_eq!(mint_id("Infection rate", IdSpace::Item, |_| false).as_deref(), Ok("infection_rate"));
/// assert_eq!(
///     mint_id("Infection rate", IdSpace::Item, |id| id == "infection_rate").as_deref(),
///     Ok("infection_rate_2")
/// );
/// assert!(mint_id("Grid width", IdSpace::Item, |_| false).is_err());
/// ```
///
/// # Errors
///
/// Returns [`SheetEditError::Reserved`] when the id minted from the name, or a suffixed id tried against a clash, is
/// reserved in `space`. "Henad" mints `henad_2` when `henad` is taken, and `henad_` starts every reserved item id.
pub fn mint_id(name: &str, space: IdSpace, is_taken: impl Fn(&str) -> bool) -> Result<String, SheetEditError> {
    let mut base = String::new();
    let mut gap = false;
    for character in name.chars() {
        let character = character.to_ascii_lowercase();
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if gap && !base.is_empty() {
                base.push('_');
            }
            gap = false;
            base.push(character);
        } else {
            gap = true;
        }
    }
    if base.is_empty() || base.starts_with(|character: char| character.is_ascii_digit()) {
        base.insert(0, 'x');
    }
    let base = shortened(&base, MAX_SHEET_ID_BYTES);
    if space.is_reserved(&base) {
        return Err(SheetEditError::Reserved(base));
    }
    let mut id = base.clone();
    let mut count = 2_u32;
    while is_taken(&id) {
        let suffix = format!("_{count}");
        id = format!("{}{suffix}", shortened(&base, MAX_SHEET_ID_BYTES - suffix.len()));
        count += 1;
        if space.is_reserved(&id) {
            return Err(SheetEditError::Reserved(id));
        }
    }
    Ok(id)
}

/// Returns `id`, an ASCII id, cut to at most `bytes` bytes with no `_` at its end.
fn shortened(id: &str, bytes: usize) -> String {
    id.get(..bytes.min(id.len()))
        .unwrap_or(id)
        .trim_end_matches('_')
        .to_owned()
}
