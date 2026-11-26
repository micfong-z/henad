//! Choices a sheet cannot run in this version, as tests an editor runs before it offers a choice.
//!
//! The checker and the lowering refuse each of these choices through the same tests, so an editor that greys a choice
//! one of these refuses and the build of the sheet never disagree. Each test returns the refusal in the author's
//! words, the message the build gives, or `None` for a choice the sheet runs.
//!
//! A *Move by* is a Move line that moves agents by an arrow, and a *Move to* one that moves them to a place. Beside
//! walls and a ground that blocks a state, a Move line with no mode yet counts as a Move by, as the lowering counts it.
//!
//! ```
//! use henad_graph::sheet::{ParsedSheet, refusals, starters};
//!
//! let walk = starters().iter().find(|starter| starter.file_name == "random_walk.toml").expect("the starter");
//! let sheet = ParsedSheet::parse(walk.text).expect("the starter reads");
//! // The walkers move by an arrow, so walls around them are refused.
//! assert!(refusals::walls(sheet.document()).is_some());
//! ```

use super::document::{CellWhich, Edges, LineKind, LineMode, MadeOf, PropertyType, SheetDocument, SheetLine};
use super::relations::CardRelations;
use super::sentence;

/// Returns whether a line of `kind` looks at the cells or agents around its place: a Count, a Look around or a Pick a
/// cell.
///
/// Neither a Repeat nor a card that runs once can hold such a line.
pub fn looks_around(kind: LineKind) -> bool {
    matches!(kind, LineKind::Count | LineKind::LookAround | LineKind::PickCell)
}

/// Returns whether `line` makes a change that a Repeat cannot hold in this version: a Keep between, When, Repeat,
/// Leave, Record or Move to.
pub fn changes_outside_repeat(line: &SheetLine) -> bool {
    matches!(
        line.kind,
        LineKind::KeepBetween | LineKind::When | LineKind::Repeat | LineKind::Leave | LineKind::Record
    ) || (line.kind == LineKind::Move && line.mode == Some(LineMode::To))
}

/// Returns the refusal of `line` inside a Repeat, or inside a block inside one, `None` for a line a Repeat holds.
///
/// A line that looks around ([`looks_around`]) takes [`sentence::LOOK_IN_REPEAT`], which the checker gives, and a
/// change a Repeat cannot hold ([`changes_outside_repeat`]) [`sentence::NOT_YET_REPEAT_CHANGE`], which the lowering
/// gives.
pub fn in_repeat(line: &SheetLine) -> Option<&'static str> {
    if looks_around(line.kind) {
        Some(sentence::LOOK_IN_REPEAT)
    } else if changes_outside_repeat(line) {
        Some(sentence::NOT_YET_REPEAT_CHANGE)
    } else {
        None
    }
}

/// Returns whether `line` is a Move by.
pub(crate) fn is_move_by(line: &SheetLine) -> bool {
    line.kind == LineKind::Move && line.mode != Some(LineMode::To)
}

/// Returns the first Move by of `document`, as the index of its card and its id, in the order of the cards and each
/// card's display order.
pub fn first_move_by(document: &SheetDocument) -> Option<(usize, &str)> {
    document.cards.iter().enumerate().find_map(|(card_index, card)| {
        let relations = CardRelations::new(document, card);
        relations
            .order()
            .iter()
            .find(|id| relations.line(id).is_some_and(is_move_by))
            .map(|id| (card_index, *id))
    })
}

/// Returns whether a card of `document` moves agents to places. The Heading of such agents is a step between cells.
pub fn moves_to_cells(document: &SheetDocument) -> bool {
    document
        .cards
        .iter()
        .flat_map(|card| card.line_tables.values())
        .any(|line| line.kind == LineKind::Move && line.mode == Some(LineMode::To))
}

/// Returns the refusal of walls around the world of `document`, [`sentence::NOT_YET_AGENT_WALLS`] for a world of
/// agents with a Move by, and `None` otherwise.
pub fn walls(document: &SheetDocument) -> Option<&'static str> {
    (document.world.made_of == MadeOf::Agents && first_move_by(document).is_some())
        .then_some(sentence::NOT_YET_AGENT_WALLS)
}

/// Returns the refusal of a ground state that agents cannot enter, [`sentence::NOT_YET_BLOCKED_MOVE_BY`] while
/// `document` holds a Move by, and `None` otherwise.
pub fn blocked_state(document: &SheetDocument) -> Option<&'static str> {
    first_move_by(document).map(|_| sentence::NOT_YET_BLOCKED_MOVE_BY)
}

/// Returns the refusal of a look at the cell ahead of an agent of `document`, [`sentence::AHEAD_NEEDS_CELLS`] unless
/// the agents have a Heading and a card moves them to places, and `None` otherwise.
pub fn cell_ahead(document: &SheetDocument) -> Option<&'static str> {
    let heading = document.agents.first().is_some_and(|kind| {
        kind.properties
            .iter()
            .any(|property| property.property_type == PropertyType::Heading)
    });
    let moves = document.world.made_of == MadeOf::Agents && moves_to_cells(document);
    (!(heading && moves)).then_some(sentence::AHEAD_NEEDS_CELLS)
}

/// Returns the refusal of the change `line` on the card of `relations` in `document`, in the place of line `except`, or
/// at the end of the card without one, `None` for a change the sheet runs there.
///
/// A card changes a property one way: a Set beside a Steer of one property, or a Move by beside a Move to, is refused
/// on the lower line, naming the higher one ([`sentence::set_already`], [`sentence::steered_already`],
/// [`sentence::moved_by_already`], [`sentence::moved_to_already`]). In the place of a line inside a Repeat, `line`
/// takes the refusal of [`in_repeat`]. A Move by also takes the refusal of walls around
/// agents ([`sentence::NOT_YET_AGENT_WALLS`]) or of a ground that blocks a state
/// ([`sentence::NOT_YET_BLOCKED_MOVE_BY`]), and a line in the place of the sheet's last Move to that moves agents no
/// longer to places takes [`sentence::AHEAD_NEEDS_CELLS`] while a line looks at the cell ahead.
pub fn change(
    document: &SheetDocument,
    relations: &CardRelations<'_>,
    line: &SheetLine,
    except: Option<&str>,
) -> Option<String> {
    if let Some((way, property)) = Way::of(line) {
        let order = relations.order();
        let at = except.and_then(|id| relations.index(id)).unwrap_or(order.len());
        let conflict = order
            .iter()
            .enumerate()
            .filter(|(_, id)| Some(**id) != except)
            .find_map(|(index, id)| {
                let (known, changed) = relations.line(id).and_then(Way::of)?;
                (changed == property && way.conflicts_with(known)).then_some((index, known))
            });
        if let Some((index, known)) = conflict {
            let name = property.map_or_else(String::new, |id| property_name(document, id).to_owned());
            let prefix = |index: usize| sentence::line_prefix(relations.button(), index + 1);
            return Some(if index < at {
                known.conflict(&prefix(index), &name)
            } else {
                way.conflict(&prefix(at), &name)
            });
        }
    }
    if let Some(refusal) = except
        .filter(|id| relations.repeat_of(id).is_some())
        .and_then(|_| in_repeat(line))
    {
        return Some(refusal.to_owned());
    }
    let agents = document.world.made_of == MadeOf::Agents;
    if is_move_by(line) {
        if agents && document.world.edges == Edges::Walls {
            return Some(sentence::NOT_YET_AGENT_WALLS.to_owned());
        }
        if document
            .ground
            .as_ref()
            .is_some_and(|ground| !ground.blocked.is_empty())
        {
            return Some(sentence::NOT_YET_BLOCKED_MOVE_BY.to_owned());
        }
    }
    let lines = || document.cards.iter().flat_map(|card| card.line_tables.iter());
    let is_move_to = |line: &SheetLine| line.kind == LineKind::Move && line.mode == Some(LineMode::To);
    let replaced_move_to = except.is_some_and(|except| {
        lines().any(|(id, known)| id == except && is_move_to(known))
            && !lines().any(|(id, known)| id != except && is_move_to(known))
    });
    let looks_ahead =
        lines().any(|(_, known)| known.kind == LineKind::LookAtCell && known.which == Some(CellWhich::Ahead));
    (agents && replaced_move_to && !is_move_to(line) && looks_ahead).then(|| sentence::AHEAD_NEEDS_CELLS.to_owned())
}

/// Returns the name of the property `id` of the cells, a kind of agent or the ground of `document`, or `id` itself when
/// no property has that id.
fn property_name<'a>(document: &'a SheetDocument, id: &'a str) -> &'a str {
    let ground = document.ground.iter().flat_map(|ground| &ground.properties);
    let agents = document.agents.iter().flat_map(|kind| &kind.properties);
    document
        .cells
        .iter()
        .flatten()
        .chain(agents)
        .chain(ground)
        .find(|property| property.id == id)
        .map_or(id, |property| property.name.as_str())
}

/// Way a line changes a property, for the rule that a card changes a property one way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Way {
    /// Set.
    Set,
    /// Steer.
    Steer,
    /// Move by an arrow.
    MoveBy,
    /// Move to a place.
    MoveTo,
}

impl Way {
    /// Returns the way `line` changes a property, with the id of the property a Set or Steer names, `None` for a line
    /// of another kind.
    pub(crate) fn of(line: &SheetLine) -> Option<(Self, Option<&str>)> {
        match line.kind {
            LineKind::Set => Some((Self::Set, Some(line.property.as_deref()?))),
            LineKind::Steer => Some((Self::Steer, Some(line.property.as_deref()?))),
            LineKind::Move => match line.mode? {
                LineMode::To => Some((Self::MoveTo, None)),
                _ => Some((Self::MoveBy, None)),
            },
            _ => None,
        }
    }

    /// Returns the error on a line that changes `property` another way than the line whose line prefix is `prefix`,
    /// such as "Line 3".
    ///
    /// That line changes the property this way.
    pub(crate) fn conflict(self, prefix: &str, property: &str) -> String {
        match self {
            Self::Set => sentence::set_already(prefix, property),
            Self::Steer => sentence::steered_already(prefix, property),
            Self::MoveBy => sentence::moved_by_already(prefix),
            Self::MoveTo => sentence::moved_to_already(prefix),
        }
    }

    /// Returns whether a change `self` and a change `other` of one property conflict.
    pub(crate) fn conflicts_with(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Set, Self::Steer)
                | (Self::Steer, Self::Set)
                | (Self::MoveBy, Self::MoveTo)
                | (Self::MoveTo, Self::MoveBy)
        )
    }
}
