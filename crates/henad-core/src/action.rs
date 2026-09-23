//! One-off steps a model offers the user, run between ticks.

use crate::authoring::primitives::rng::mix_seed;

/// An action a model declares.
///
/// The Parameters panel draws a button per entry and `henad-cli` names one with `--act`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionDescriptor {
    /// Stable name, what `--act` matches on.
    pub id: &'static str,
    /// Button label.
    pub label: &'static str,
}

impl ActionDescriptor {
    pub const fn new(id: &'static str, label: &'static str) -> Self {
        Self { id, label }
    }
}

/// Domain separator for the action stream.
const ACTION_SALT: u64 = 0x00AC_7104_5EED_0001;

/// Where a state's action stream starts, from the seed the state was built with.
///
/// Kept apart from the tick stream. Otherwise a press draws the numbers the next tick would have.
pub fn action_seed(seed: Option<u64>) -> u64 {
    mix_seed(seed.unwrap_or(0) ^ ACTION_SALT)
}

/// Declares a model's actions and their indices in one place.
///
/// The index is the declaration's position, so it is derived rather than written down. Expands at
/// module scope, next to the impl that forwards `ACTIONS` to `ACTION_SPECS`.
///
/// ```ignore
/// actions! {
///     const RANDOMISE = ActionDescriptor::new("randomise", "Randomise");
///     const CLEAR = ActionDescriptor::new("clear", "Clear");
/// }
/// ```
#[macro_export]
macro_rules! actions {
    ($($(#[$meta:meta])* $vis:vis const $name:ident = $descriptor:expr;)+) => {
        $crate::__indices!(0usize, $([$(#[$meta])* $vis $name],)+);

        /// This model's actions, in index order.
        const ACTION_SPECS: &[$crate::action::ActionDescriptor] = &[$($descriptor),+];
    };
}

#[cfg(test)]
mod tests {
    /// The macro has to expand in function scope as well as module scope (C-ANYWHERE).
    #[test]
    fn actions_macro_numbers_entries_in_declaration_order() {
        crate::actions! {
            const RANDOMISE = crate::action::ActionDescriptor::new("randomise", "Randomise");
            /// An entry can carry a doc comment.
            const CLEAR = crate::action::ActionDescriptor::new("clear", "Clear");
        }
        assert_eq!((RANDOMISE, CLEAR), (0, 1), "indices follow declaration order");
        assert_eq!(ACTION_SPECS.len(), 2);
        assert_eq!(ACTION_SPECS[CLEAR].label, "Clear");
    }

    /// Two states built from one seed must agree, and two from different seeds must not.
    #[test]
    fn the_action_stream_is_seeded_and_apart_from_the_tick_stream() {
        use crate::action::action_seed;
        assert_eq!(action_seed(Some(7)), action_seed(Some(7)));
        assert_ne!(action_seed(Some(7)), action_seed(Some(8)));
        assert_ne!(action_seed(Some(7)), crate::authoring::primitives::rng::mix_seed(7));
        assert_ne!(action_seed(None), 0, "an unseeded state still needs a usable state");
    }
}
