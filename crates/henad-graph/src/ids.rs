//! Stable ids of a graph file and the grammars of its names.

use std::fmt;
use std::str::FromStr;

/// Stable id of a node, written `n<k>` with `k >= 1` and no leading zero.
///
/// Ids order by `k`, the natural order the canonical text sorts nodes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// Returns the id with number `number`, or `None` for 0.
    pub fn new(number: u32) -> Option<Self> {
        (number >= 1).then_some(Self(number))
    }

    /// Number `k` of the id `n<k>`.
    pub fn number(self) -> u32 {
        self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "n{}", self.0)
    }
}

impl FromStr for NodeId {
    type Err = String;

    /// Reads an id written `n<k>`.
    ///
    /// # Errors
    ///
    /// Returns the message for text of another form. `n0`, `n01` and a number past `u32::MAX` are refused.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let refused = || format!("'{text}' is no node id, which is `n` and a number from 1, with no leading zero");
        let digits = text.strip_prefix('n').ok_or_else(refused)?;
        if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(refused());
        }
        digits.parse().map(Self).map_err(|_error| refused())
    }
}

/// Returns whether `text` follows the id grammar `[a-z][a-z0-9_]{0,63}`, with no double underscore.
///
/// An id holds no double underscore, which keeps generated WGSL names injective.
pub fn is_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes.len() <= 64
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
        && !text.contains("__")
}

/// Returns whether `text` follows the port segment grammar `[A-Za-z][A-Za-z0-9_]{0,63}`.
pub fn is_port_segment(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.len() <= 64
        && bytes.iter().all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
}
