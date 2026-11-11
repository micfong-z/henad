//! Edit commands on a sheet and the builder that runs them: one command per edit of the sheet editor, with no user
//! interface.
//!
//! A [`SheetBuilder`] holds a [`SheetDocument`] and the [`KeyStream`] new draws take their keys from.
//! [`SheetBuilder::apply`] runs one [`SheetEdit`] on a copy. A refused command changes nothing. A command checks the
//! structure it changes (ids, names, the line a slot path reaches) and leaves the meaning to [`super::check`].
//!
//! - **Ids.** A new item's id is minted once from its name by [`mint_id`]: lower case, each run of other characters
//!   as one `_`, a leading `x` before a digit, `_2`, `_3` and so on against a clash, and a reserved id refused. A
//!   rename changes the name alone, and [`SheetEdit::ChangeId`] rewrites every reference. A card that runs each tick is
//!   `cell_tick` or `<agent kind id>_tick`, and the card of a button that runs once takes the button's id.
//! - **Lines.** A new line takes the id `l<next_line>`, and `next_line` grows past it. A deleted line's number is never
//!   minted again. A line that draws takes a fresh key, distinct from every key of the sheet, and keeps it for its
//!   life, also while an edit takes its draw away. [`SheetEdit::DrawNewNumbers`] mints a new one.
//! - **Deletes.** A delete never refuses. It clears every reference to what it deletes: a slot that named it turns
//!   blank, a field turns empty, and a table about it, such as a measure of a deleted state, goes with it. A start
//!   that named a deleted state takes the first state its property still holds. Note that the checker reads most
//!   blanks of a card line as a skip of the line, and a blank outside the card lines, such as a start's chance, as an
//!   error. Deleting a When keeps its lines one level up.
//! - **Names.** After every command, each result line that follows its automatic name takes it again, made unique, as
//!   [`automatic_name`] gives it.
//! - **Rates.** A new rate takes the unit of one tick, marked as taken from the tick. A parameter put into a rate whose
//!   unit is empty or marked gives the rate its own unit and clears the mark. A unit chosen by hand clears the mark
//!   too, and gives a parameter with no unit that the rate reads the same unit. [`SheetEdit::ChangeToRate`] and
//!   [`SheetEdit::ChangeToChance`] turn a typed chance into a rate and back, keeping the chance per tick.
//!
//! ```
//! use henad_graph::sheet::ParsedSheet;
//! use henad_graph::sheet::document::ParameterType;
//! use henad_graph::sheet::edit::{SheetBuilder, SheetEdit};
//!
//! let sheet = ParsedSheet::parse(henad_graph::sheet::starters()[0].text).expect("the starter reads");
//! let mut builder = SheetBuilder::new(sheet.document().clone()).with_key_seed(1);
//! let made = builder
//!     .apply(SheetEdit::AddParameter {
//!         name: "Growth rate".to_owned(),
//!         parameter_type: ParameterType::Number,
//!         min: 0.0,
//!         max: 1.0,
//!         default: 0.1,
//!         unit: None,
//!     })
//!     .expect("the parameter is added");
//! assert_eq!(made, ["growth_rate"]);
//! let written = builder.document().to_text().expect("the sheet writes");
//! assert!(written.contains("id = \"growth_rate\""));
//! ```

mod colors;
mod command;
mod declarations;
mod ids;
mod line_commands;
pub(crate) mod lines;
mod names;
mod references;

pub use colors::{MEASURE_COLORS, PALETTE_COLORS, STATE_COLORS, suggested_state_color};
pub use command::{LineField, MoveDirection, SheetEdit, SheetEditError, SheetItem};
pub use ids::{IdSpace, is_reserved_name, mint_id};
pub(crate) use line_commands::unit_in_text;
pub use lines::slot_value;
pub use names::{automatic_name, automatic_summary_name};

use std::collections::BTreeSet;
use std::fmt;

use self::references::{all_properties, all_properties_mut};
use super::check::holds_draw;
use super::document::{
    AgentKind, LineKind, MadeOf, MeasureRule, SheetDocument, SheetLine, SheetMeasure, SheetProperty, SheetValue,
    StartRule, StartsAs,
};
use super::limits::MAX_NESTING;
use super::sentence::{self, ANY_STATE_ID};
use crate::keys::KeyStream;

/// A sheet under edit: its document and the stream of keys new draws take.
#[derive(Clone)]
pub struct SheetBuilder {
    /// The sheet.
    document: SheetDocument,
    /// Stream new keys come from.
    keys: KeyStream,
}

/// Prints the sheet's id and the number of its cards.
impl fmt::Debug for SheetBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SheetBuilder")
            .field("id", &self.document.sheet.id)
            .field("cards", &self.document.cards.len())
            .finish_non_exhaustive()
    }
}

impl SheetBuilder {
    /// Returns a builder of `document`, with keys seeded from entropy.
    pub fn new(document: SheetDocument) -> Self {
        Self {
            document,
            keys: KeyStream::from_entropy(),
        }
    }

    /// Returns the builder with its keys drawn from `seed`, so a test mints the same keys every run.
    pub fn with_key_seed(mut self, seed: u64) -> Self {
        self.keys = KeyStream::seeded(seed);
        self
    }

    /// Sheet under edit.
    pub fn document(&self) -> &SheetDocument {
        &self.document
    }

    /// Returns the sheet under edit.
    pub fn into_document(self) -> SheetDocument {
        self.document
    }

    /// Runs `edit` and returns the ids of the items it made, empty for an edit that makes none.
    ///
    /// # Errors
    ///
    /// Returns the reason the edit is refused. On an error nothing changes. An edit that would leave a number that is
    /// not finite anywhere in a sheet that held none is refused with [`sentence::number_not_kept`].
    pub fn apply(&mut self, edit: SheetEdit) -> Result<Vec<String>, SheetEditError> {
        let mut draft = Draft {
            document: self.document.clone(),
            keys: self.keys.clone(),
            touched: Vec::new(),
            touched_starts: Vec::new(),
        };
        let made = draft.run(edit)?;
        draft.finish();
        names::update_summary_names(&self.document, &mut draft.document);
        names::update_names(&self.document, &mut draft.document);
        // A sheet the writer cannot write, one holding a number that is not finite, would leave no text to undo to. A
        // sheet that held such a number before the edit takes edits as before.
        if let Err(error) = draft.document.to_text()
            && self.document.to_text().is_ok()
        {
            return Err(SheetEditError::Invalid(
                sentence::number_not_kept(error.value).to_owned(),
            ));
        }
        self.document = draft.document;
        self.keys = draft.keys;
        Ok(made)
    }
}

/// A copy of the sheet that one edit changes, with the lines it touched.
struct Draft {
    /// The sheet.
    document: SheetDocument,
    /// Stream new keys come from.
    keys: KeyStream,
    /// Ids of the lines the edit added or changed, whose keys are checked when it ends.
    touched: Vec<String>,
    /// Ids of the properties whose starts the edit set, whose keys are checked when it ends.
    touched_starts: Vec<String>,
}

/// Returns an error for a missing `what` named `id`.
fn missing(what: &str, id: &str) -> SheetEditError {
    SheetEditError::Missing(format!("the sheet has no {what} '{id}'"))
}

/// Returns `name` trimmed, or an error naming `what` for an empty name.
fn clean_name(name: &str, what: &str) -> Result<String, SheetEditError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(SheetEditError::Invalid(format!("a {what} needs a name")));
    }
    Ok(name.to_owned())
}

/// Returns an error when `name` equals one of `names`, case aside.
fn refuse_taken<'a>(name: &str, mut names: impl Iterator<Item = &'a str>, what: &str) -> Result<(), SheetEditError> {
    let lower = name.to_lowercase();
    if names.any(|other| other.to_lowercase() == lower) {
        return Err(SheetEditError::Taken(format!(
            "a {what} called '{name}' already exists"
        )));
    }
    Ok(())
}

/// Returns an error for the first number of `numbers` that is not finite.
fn refuse_infinite(numbers: &[f64]) -> Result<(), SheetEditError> {
    match numbers.iter().find(|number| !number.is_finite()) {
        None => Ok(()),
        Some(number) => Err(SheetEditError::Invalid(sentence::number_not_kept(*number).to_owned())),
    }
}

/// Returns whether a start of `starts`, a start of a property of `document`, draws random numbers.
///
/// A start on the ground draws one of the cells its state is painted on when there are two or more.
fn starts_draws(document: &SheetDocument, starts: &StartsAs) -> bool {
    matches!(starts.rule, StartRule::ByChance | StartRule::Anywhere)
        || matches!(starts.value.value(), Some(SheetValue::Phrase(phrase)) if holds_draw(phrase))
        || starts.rule == StartRule::OnGround
            && all_properties(document)
                .find(|property| Some(property.id.as_str()) == starts.property.as_deref())
                .and_then(|property| property.starts_as.as_ref())
                .is_some_and(|painted| painted.single_cells(starts.state.as_deref().unwrap_or("")) > 1)
}

/// Depth of a line table in a sheet's text: the cards, the card, `line` and the line's id.
const LINE_TABLE_DEPTH: usize = 4;

impl Draft {
    /// Runs one edit on the draft.
    #[expect(clippy::too_many_lines, reason = "one arm per edit reads best as one table")]
    fn run(&mut self, edit: SheetEdit) -> Result<Vec<String>, SheetEditError> {
        match edit {
            SheetEdit::SetSheetName { name } => {
                self.document.sheet.name = clean_name(&name, "model")?;
            }
            SheetEdit::SetPurpose { purpose } => {
                self.document.sheet.purpose = purpose.filter(|purpose| !purpose.trim().is_empty());
            }
            SheetEdit::SetWorld { width, height, edges } => {
                if width == 0 || height == 0 {
                    return Err(SheetEditError::Invalid(
                        "the world is at least 1 cell wide and 1 cell high".to_owned(),
                    ));
                }
                self.document.world.width = width;
                self.document.world.height = height;
                self.document.world.edges = edges;
            }
            SheetEdit::SetTime { time } => {
                if let Some(time) = time {
                    refuse_infinite(&[time.tick_length])?;
                }
                if let Some(time) = time
                    && time.tick_length <= 0.0
                {
                    return Err(SheetEditError::Invalid(
                        "one tick lasts a number of units above 0".to_owned(),
                    ));
                }
                self.document.time = time;
            }
            SheetEdit::AddProperty {
                thing,
                name,
                property_type,
            } => return self.add_property(&thing, &name, property_type),
            SheetEdit::SetPropertyType {
                property,
                property_type,
            } => self.set_property_type(&property, property_type)?,
            SheetEdit::AddState { property, name, color } => return self.add_state(&property, &name, color),
            SheetEdit::AddParameter {
                name,
                parameter_type,
                min,
                max,
                default,
                unit,
            } => return self.add_parameter(&name, parameter_type, min, max, default, unit),
            SheetEdit::SetParameter {
                parameter,
                min,
                max,
                default,
                step,
                unit,
                means,
            } => {
                refuse_infinite(&[min, max, default, step.unwrap_or(0.0)])?;
                let parameter = self
                    .document
                    .parameters
                    .iter_mut()
                    .find(|known| known.id == parameter)
                    .ok_or_else(|| missing("parameter", &parameter))?;
                parameter.min = min;
                parameter.max = max;
                parameter.default = default;
                parameter.step = step;
                parameter.unit = unit.filter(|unit| !unit.trim().is_empty());
                parameter.means = means.filter(|means| !means.trim().is_empty());
            }
            SheetEdit::AddMeasure {
                name,
                rule,
                over,
                property,
                state,
                event,
                value,
                color,
            } => {
                let name = clean_name(&name, "measure")?;
                refuse_taken(
                    &name,
                    self.document.measures.iter().map(|measure| measure.name.as_str()),
                    "measure",
                )?;
                let state_color = match (&property, &state) {
                    (Some(property), Some(state)) if rule == MeasureRule::CountInState => self
                        .property(property)
                        .and_then(|property| property.states.iter().find(|known| known.id == *state))
                        .map(|state| state.color),
                    _ => None,
                };
                let color = color
                    .or(state_color)
                    .unwrap_or_else(|| MEASURE_COLORS[self.document.measures.len() % MEASURE_COLORS.len()]);
                let id = self.mint_item(&name)?;
                let mut measure = SheetMeasure::new(id.clone(), name, color, rule);
                measure.over = over;
                measure.property = property;
                measure.state = state;
                measure.event = event;
                measure.value = value;
                self.document.measures.push(measure);
                return Ok(vec![id]);
            }
            SheetEdit::AddEvent { name } => return self.add_event(&name),
            SheetEdit::AddButton { name, rule, thing } => return self.add_button(&name, rule, &thing),
            SheetEdit::SetButtonSteps { button, does } => self.set_button_steps(&button, does)?,
            SheetEdit::AddCard { every } => return self.add_card(&every),
            SheetEdit::Rename { item, name } => self.rename(&item, &name)?,
            SheetEdit::ChangeId { item, id } => self.change_id(&item, &id)?,
            SheetEdit::Recolor { item, color } => self.recolor(&item, color)?,
            SheetEdit::Delete { item } => {
                self.delete(&item)?;
                // A ground with nothing left on it goes, as it came with its first property.
                if self
                    .document
                    .ground
                    .as_ref()
                    .is_some_and(|ground| ground.properties.is_empty() && ground.blocked.is_empty())
                {
                    self.document.ground = None;
                }
            }
            SheetEdit::SetStartsAs { property, starts_as } => {
                self.touched_starts.push(property.clone());
                let property = self.property_mut(&property)?;
                let old_key = property.starts_as.as_ref().and_then(|starts| starts.key);
                property.starts_as = starts_as.map(|starts| {
                    let mut starts = *starts;
                    starts.key = starts.key.or(old_key);
                    starts
                });
            }
            SheetEdit::SetColorRule { thing, rule } => {
                self.require_thing(&thing)?;
                match rule {
                    Some(rule) => {
                        self.document.colors.insert(thing, rule);
                    }
                    None => {
                        self.document.colors.remove(&thing);
                    }
                }
            }
            SheetEdit::SetAgentKind { kind, count, max_count } => {
                if count == 0 || count > max_count {
                    return Err(SheetEditError::Invalid(format!(
                        "the agents '{kind}' need a count from 1 to the largest number the world holds"
                    )));
                }
                let kind = self.agent_kind_mut(&kind)?;
                kind.count = count;
                kind.max_count = max_count;
            }
            SheetEdit::SetMarks {
                property,
                marks,
                kept_per_tick,
            } => self.set_marks(&property, marks, kept_per_tick)?,
            SheetEdit::SetBlocked {
                property,
                state,
                blocked,
            } => self.set_blocked(&property, &state, blocked)?,
            SheetEdit::AddLine { card, inside, line } => return self.add_line(&card, inside.as_deref(), *line),
            SheetEdit::SetSlot { line, path, value } => self.set_slot(&line, &path, value)?,
            SheetEdit::SetField { line, phrase, field } => self.set_field(&line, &phrase, field)?,
            SheetEdit::ChooseCondition { line, item } => self.choose_condition(&line, item)?,
            SheetEdit::ChooseRowCondition { line, row, item } => self.choose_row_condition(&line, row, item)?,
            SheetEdit::UseOneSide { line } => self.use_one_side(&line)?,
            SheetEdit::UseCountForEach { line } => self.use_count_for_each(&line)?,
            SheetEdit::MoveConditionInto { when } => self.move_condition_into(&when)?,
            SheetEdit::RenameResult { line, name } => self.rename_result(&line, &name)?,
            SheetEdit::AddSummary { line, summary } => self.add_summary(&line, summary)?,
            SheetEdit::DeleteSummary { line, index } => self.delete_summary(&line, index)?,
            SheetEdit::SetNote { line, note } => {
                self.line_mut(&line)?.note = note.filter(|note| !note.trim().is_empty());
            }
            SheetEdit::MoveLine { line, direction } => {
                let card = self.card_of_line(&line)?;
                lines::move_line(&mut self.document.cards[card], &line, direction)?;
            }
            SheetEdit::DeleteLine { line } => self.delete_line(&line)?,
            SheetEdit::ChangeToRate { line } => self.change_draw(&line, LineKind::Rate)?,
            SheetEdit::ChangeToChance { line } => self.change_draw(&line, LineKind::Chance)?,
            SheetEdit::DrawNewNumbers { line } => {
                let target = self.line(&line)?;
                if !holds_draw(target) {
                    return Err(SheetEditError::Invalid(format!("line '{line}' draws no numbers")));
                }
                let key = self.mint_key();
                self.line_mut(&line)?.key = Some(key);
            }
        }
        Ok(Vec::new())
    }

    /// Gives every touched line that draws a key no other draw holds, and every touched start that draws or shares its
    /// key one too.
    fn finish(&mut self) {
        let touched = std::mem::take(&mut self.touched);
        for id in touched {
            let Some((card, line)) = self.document.line(&id) else {
                continue;
            };
            if !holds_draw(line) {
                continue;
            }
            let shared = line.key.is_some_and(|key| self.key_count(key) > 1);
            if line.key.is_none() || shared {
                let key = self.mint_key();
                if let Some(line) = self.document.cards[card].line_tables.get_mut(&id) {
                    line.key = Some(key);
                }
            }
        }
        let mut touched_starts = std::mem::take(&mut self.touched_starts);
        // A start on a painted ground property is touched with the ground's start, which decides whether it draws.
        let on_touched_ground: Vec<String> = all_properties(&self.document)
            .filter(|property| {
                property.starts_as.as_ref().is_some_and(|starts| {
                    starts.rule == StartRule::OnGround
                        && starts
                            .property
                            .as_ref()
                            .is_some_and(|ground| touched_starts.contains(ground))
                })
            })
            .map(|property| property.id.clone())
            .collect();
        touched_starts.extend(on_touched_ground);
        let rekeyed: Vec<String> = all_properties(&self.document)
            .filter(|property| {
                touched_starts.contains(&property.id)
                    && property.starts_as.as_ref().is_some_and(|starts| match starts.key {
                        None => starts_draws(&self.document, starts),
                        Some(key) => self.key_count(key) > 1,
                    })
            })
            .map(|property| property.id.clone())
            .collect();
        for id in rekeyed {
            let key = self.mint_key();
            if let Ok(property) = self.property_mut(&id)
                && let Some(starts) = property.starts_as.as_mut()
            {
                starts.key = Some(key);
            }
        }
    }

    /// Returns every key of the sheet: those of the lines and those of the starts.
    fn keys_held(&self) -> Vec<u32> {
        self.document
            .cards
            .iter()
            .flat_map(|card| card.line_tables.values())
            .filter_map(|line| line.key)
            .chain(all_properties(&self.document).filter_map(|property| property.starts_as.as_ref()?.key))
            .collect()
    }

    /// Returns the number of draws that hold `key`.
    fn key_count(&self, key: u32) -> usize {
        self.keys_held().into_iter().filter(|held| *held == key).count()
    }

    /// Returns a fresh key, distinct from every key of the sheet.
    fn mint_key(&mut self) -> u32 {
        let taken: BTreeSet<u32> = self.keys_held().into_iter().collect();
        self.keys.next_key_not_in(|key| taken.contains(&key))
    }

    /// Returns every id of the item namespace: properties, parameters, measures and events.
    fn item_ids(&self) -> BTreeSet<String> {
        all_properties(&self.document)
            .map(|property| property.id.clone())
            .chain(self.document.parameters.iter().map(|parameter| parameter.id.clone()))
            .chain(self.document.measures.iter().map(|measure| measure.id.clone()))
            .chain(self.document.events.iter().map(|event| event.id.clone()))
            .collect()
    }

    /// Returns an id minted from `name` in the item namespace.
    fn mint_item(&self, name: &str) -> Result<String, SheetEditError> {
        let taken = self.item_ids();
        mint_id(name, IdSpace::Item, |id| taken.contains(id))
    }

    /// Returns the property `id` of any kind of thing.
    fn property(&self, id: &str) -> Option<&SheetProperty> {
        all_properties(&self.document).find(|property| property.id == id)
    }

    /// Returns the property `id` of any kind of thing, mutable.
    fn property_mut(&mut self, id: &str) -> Result<&mut SheetProperty, SheetEditError> {
        all_properties_mut(&mut self.document)
            .find(|property| property.id == id)
            .ok_or_else(|| missing("property", id))
    }

    /// Returns an error unless `thing` is a kind of thing of the sheet's world.
    fn require_thing(&self, thing: &str) -> Result<(), SheetEditError> {
        let known = match thing {
            "cells" => self.document.world.made_of == MadeOf::Cells,
            "ground" => self.document.world.made_of == MadeOf::Agents,
            _ => self.document.agents.iter().any(|kind| kind.id == thing),
        };
        if known {
            Ok(())
        } else {
            Err(missing("kind of thing", thing))
        }
    }

    /// Returns the properties of the kind of thing `thing`, made empty when the sheet writes none yet.
    fn thing_properties_mut(&mut self, thing: &str) -> Result<&mut Vec<SheetProperty>, SheetEditError> {
        self.require_thing(thing)?;
        match thing {
            "cells" => Ok(self.document.cells.get_or_insert_with(Vec::new)),
            "ground" => Ok(&mut self.document.ground.get_or_insert_with(Default::default).properties),
            _ => self
                .document
                .agents
                .iter_mut()
                .find(|kind| kind.id == thing)
                .map(|kind| &mut kind.properties)
                .ok_or_else(|| missing("kind of thing", thing)),
        }
    }

    /// Returns the index of the card that holds line `line_id`.
    fn card_of_line(&self, line_id: &str) -> Result<usize, SheetEditError> {
        self.document
            .line(line_id)
            .map(|(card, _)| card)
            .ok_or_else(|| missing("line", line_id))
    }

    /// Returns line `line_id`.
    fn line(&self, line_id: &str) -> Result<&SheetLine, SheetEditError> {
        self.document
            .line(line_id)
            .map(|(_, line)| line)
            .ok_or_else(|| missing("line", line_id))
    }

    /// Returns line `line_id`, mutable, and records it as touched.
    fn line_mut(&mut self, line_id: &str) -> Result<&mut SheetLine, SheetEditError> {
        let card = self.card_of_line(line_id)?;
        self.touched.push(line_id.to_owned());
        self.document.cards[card]
            .line_tables
            .get_mut(line_id)
            .ok_or_else(|| missing("line", line_id))
    }

    /// Returns the kind of agent `id`, mutable.
    fn agent_kind_mut(&mut self, id: &str) -> Result<&mut AgentKind, SheetEditError> {
        self.document
            .agents
            .iter_mut()
            .find(|kind| kind.id == id)
            .ok_or_else(|| missing("kind of agent", id))
    }

    /// Returns an error when the tables and lists of line `line_id` nest past what a sheet reads.
    fn refuse_deep(&self, line_id: &str) -> Result<(), SheetEditError> {
        let line = self.line(line_id)?;
        if LINE_TABLE_DEPTH + lines::line_depth(line) > MAX_NESTING {
            return Err(SheetEditError::Invalid(format!(
                "line '{line_id}' would nest tables and lists past {MAX_NESTING}"
            )));
        }
        Ok(())
    }
}
