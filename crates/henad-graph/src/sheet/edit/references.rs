//! Every place a sheet names an item by id, and the two changes made to all of them at once: a new id, or a clear.
//!
//! A *target* is the item whose references change. A *clear* leaves a blank where a slot named the target, takes the
//! target out of a list, empties a field that named it, and drops a table that is about it, such as a measure that
//! counts a deleted state. A card line whose slot turns blank does not run. A start that named a deleted state of its
//! own property, or of the ground property it starts on, takes the first state that property still holds.
//!
//! A state chip names a state of the property its slot is about: the property a Become changes, the property on the
//! left of a comparison, or the property of a start. A chip whose property no slot names belongs to the one property
//! that holds a state of that id, and to none when two properties hold one.

use std::collections::HashMap;

use super::lines::{SLOT_KEYS, slots_mut};
use crate::sheet::document::{SheetChip, SheetDocument, SheetLine, SheetProperty, SheetValue, Slot};

/// An item whose references change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Target {
    /// A parameter, by id.
    Parameter(String),
    /// A property of any kind of thing, by id.
    Property(String),
    /// A state of a property.
    State {
        /// Id of the property.
        property: String,
        /// Id of the state.
        state: String,
    },
    /// A line, by id. A line takes no new id.
    Line(String),
    /// An event, by id.
    Event(String),
    /// A card, by id.
    Card(String),
    /// A kind of agent, by id.
    Thing(String),
    /// A summary of a Look around line, by the line's id and the summary's id.
    Summary {
        /// Id of the line.
        line: String,
        /// Id of the summary.
        summary: String,
    },
}

/// A change to every reference of a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Change<'a> {
    /// Every reference takes this id.
    Rename(&'a str),
    /// Every reference is cleared.
    Clear,
}

/// Applies `change` to every reference of `target` in `document`.
///
/// The target's own table is left as it is. A rename of a state runs before the state takes its new id, and a clear
/// before the state leaves its property, since a state chip finds its property through the states the properties
/// hold.
pub(super) fn change_references(document: &mut SheetDocument, target: &Target, change: Change<'_>) {
    let walker = Walker::new(document, target, change);
    walker.document(document);
}

/// Returns every property of `document`: the cells', each kind of agent's, then the ground's.
pub(super) fn all_properties(document: &SheetDocument) -> impl Iterator<Item = &SheetProperty> {
    document
        .cells
        .iter()
        .flatten()
        .chain(document.agents.iter().flat_map(|kind| &kind.properties))
        .chain(document.ground.iter().flat_map(|ground| &ground.properties))
}

/// Returns every property of `document`, mutable, in the order of [`all_properties`].
pub(super) fn all_properties_mut(document: &mut SheetDocument) -> impl Iterator<Item = &mut SheetProperty> {
    document
        .cells
        .iter_mut()
        .flatten()
        .chain(document.agents.iter_mut().flat_map(|kind| &mut kind.properties))
        .chain(document.ground.iter_mut().flat_map(|ground| &mut ground.properties))
}

/// Returns the property a comparison with `left` on its left side is about: the property of a chip of mine, of
/// theirs, or of the place a result names.
pub(super) fn compared_property(left: &Slot) -> Option<&str> {
    match left.value()? {
        SheetValue::Chip(
            SheetChip::My(id)
            | SheetChip::MyNew(id)
            | SheetChip::Their(id)
            | SheetChip::Result { property: Some(id), .. },
        ) => Some(id),
        _ => None,
    }
}

/// Applies one change to the references of one target.
struct Walker<'a> {
    /// The target.
    target: &'a Target,
    /// The change.
    change: Change<'a>,
    /// Ids of the properties that hold each state id.
    owners: HashMap<String, Vec<String>>,
    /// Ids of the states of each property, in order, by property id.
    states: HashMap<String, Vec<String>>,
    /// Ids of the summaries of each Look around line, by line id.
    summaries: HashMap<String, Vec<String>>,
}

impl<'a> Walker<'a> {
    /// Creates the walker of `change` to `target` over `document`.
    fn new(document: &SheetDocument, target: &'a Target, change: Change<'a>) -> Self {
        let mut owners: HashMap<String, Vec<String>> = HashMap::new();
        let mut states: HashMap<String, Vec<String>> = HashMap::new();
        for property in all_properties(document) {
            for state in &property.states {
                owners.entry(state.id.clone()).or_default().push(property.id.clone());
            }
            states.insert(
                property.id.clone(),
                property.states.iter().map(|state| state.id.clone()).collect(),
            );
        }
        let summaries = document
            .cards
            .iter()
            .flat_map(|card| &card.line_tables)
            .map(|(id, line)| {
                (
                    id.clone(),
                    line.summaries.iter().map(|summary| summary.id.clone()).collect(),
                )
            })
            .collect();
        Self {
            target,
            change,
            owners,
            states,
            summaries,
        }
    }

    /// Returns whether `id` names the target parameter.
    fn is_parameter(&self, id: &str) -> bool {
        matches!(self.target, Target::Parameter(target) if target == id)
    }

    /// Returns whether `id` names the target property.
    fn is_property(&self, id: &str) -> bool {
        matches!(self.target, Target::Property(target) if target == id)
    }

    /// Returns whether `id` names the target line.
    fn is_line(&self, id: &str) -> bool {
        matches!(self.target, Target::Line(target) if target == id)
    }

    /// Returns whether `id` names the target event.
    fn is_event(&self, id: &str) -> bool {
        matches!(self.target, Target::Event(target) if target == id)
    }

    /// Returns whether `id` names the target card.
    fn is_card(&self, id: &str) -> bool {
        matches!(self.target, Target::Card(target) if target == id)
    }

    /// Returns whether `id` names the target kind of agent.
    fn is_thing(&self, id: &str) -> bool {
        matches!(self.target, Target::Thing(target) if target == id)
    }

    /// Returns whether the state `state`, a state of `about` when a slot names its property, is the target state.
    fn is_state(&self, about: Option<&str>, state: &str) -> bool {
        let Target::State {
            property: target_property,
            state: target_state,
        } = self.target
        else {
            return false;
        };
        if target_state != state {
            return false;
        }
        match about {
            Some(about) => about == target_property,
            None => self
                .owners
                .get(state)
                .is_some_and(|owners| owners.len() == 1 && owners[0] == *target_property),
        }
    }

    /// Renames `id`, a reference of the target, or returns true when the reference is to be cleared.
    fn hit(&self, id: &mut String) -> bool {
        match self.change {
            Change::Rename(new) => {
                new.clone_into(id);
                false
            }
            Change::Clear => true,
        }
    }

    /// Applies the change to an optional field holding an id, when `is_target` holds for it.
    fn field(&self, field: &mut Option<String>, is_target: impl Fn(&str) -> bool) {
        if self.names_cleared(field, is_target) {
            *field = None;
        }
    }

    /// Applies the change to an optional field holding an id, when `is_target` holds for it, and returns true when the
    /// reference is to be cleared.
    fn names_cleared(&self, field: &mut Option<String>, is_target: impl Fn(&str) -> bool) -> bool {
        match field {
            Some(id) if is_target(id) => self.hit(id),
            _ => false,
        }
    }

    /// Applies the change to `value`, where `about` is the property that a state in it belongs to, and returns true
    /// when the value is to be cleared.
    fn value(&self, value: &mut SheetValue, about: Option<&str>) -> bool {
        match value {
            SheetValue::Literal(_) => false,
            SheetValue::Phrase(phrase) => {
                self.line(phrase);
                false
            }
            SheetValue::List(items) => {
                self.values(items, about);
                false
            }
            SheetValue::Chip(chip) => match chip {
                SheetChip::Param(id) if self.is_parameter(id) => self.hit(id),
                SheetChip::My(id) | SheetChip::MyNew(id) | SheetChip::Their(id) if self.is_property(id) => self.hit(id),
                SheetChip::State(id) if self.is_state(about, id) => self.hit(id),
                SheetChip::Result { line, part, property } => {
                    if self.is_line(line) {
                        return self.hit(line);
                    }
                    if let Target::Summary {
                        line: target_line,
                        summary,
                    } = self.target
                        && target_line == line
                        && let Some(part) = part
                        && part == summary
                    {
                        return self.hit(part);
                    }
                    match property {
                        Some(id) if self.is_property(id) => self.hit(id),
                        _ => false,
                    }
                }
                _ => false,
            },
        }
    }

    /// Applies the change to every value of `values`, and takes out each one to be cleared.
    fn values(&self, values: &mut Vec<SheetValue>, about: Option<&str>) {
        values.retain_mut(|value| !self.value(value, about));
    }

    /// Applies the change to the value in `slot`, and leaves the slot blank when the value is to be cleared.
    fn slot(&self, slot: &mut Slot, about: Option<&str>) {
        if let Slot::Filled(value) = slot
            && self.value(value, about)
        {
            *slot = Slot::Blank;
        }
    }

    /// Applies the change to the formula in `formula`, which a clear empties when it names the target.
    fn formula(&self, formula: &mut Option<String>) {
        let Some(text) = formula.as_deref() else {
            return;
        };
        let mut named = false;
        let rewritten = rewrite_formula(text, |head, tail| {
            let is_line_head = is_line_id(head);
            let hit = match self.target {
                Target::Parameter(id) => {
                    tail.is_none() && head == id && !is_line_head && !matches!(head, "my" | "my_new")
                }
                Target::Property(id) => {
                    let is_part = is_line_head
                        && tail.is_some_and(|tail| {
                            matches!(tail, "amount" | "open" | "count")
                                || self
                                    .summaries
                                    .get(head)
                                    .is_some_and(|summaries| summaries.iter().any(|summary| summary == tail))
                        });
                    (is_line_head || matches!(head, "my" | "my_new")) && tail == Some(id.as_str()) && !is_part
                }
                Target::Line(id) => head == id,
                Target::Summary { line, summary } => head == line && tail == Some(summary.as_str()),
                _ => false,
            };
            if !hit {
                return None;
            }
            named = true;
            match (self.change, self.target) {
                (Change::Rename(new), Target::Property(_) | Target::Summary { .. }) => Some(format!("{head}.{new}")),
                (Change::Rename(new), _) => Some(new.to_owned()),
                (Change::Clear, _) => None,
            }
        });
        match self.change {
            Change::Rename(_) => *formula = Some(rewritten),
            Change::Clear if named => *formula = None,
            Change::Clear => {}
        }
    }

    /// Applies the change to every reference in `line` and the phrases inside it.
    fn line(&self, line: &mut SheetLine) {
        if let Some(property) = line.property.as_mut()
            && self.is_property(property)
            && self.hit(property)
        {
            line.property = None;
            line.new_state = None;
            line.start_state = line.start_state.take().filter(|state| state == super::ANY_STATE_ID);
        }
        let about = line.property.clone();
        let about = about.as_deref();
        self.field(&mut line.new_state, |state| self.is_state(about, state));
        self.field(&mut line.start_state, |state| self.is_state(about, state));
        self.field(&mut line.agent_kind, |id| self.is_thing(id));
        self.field(&mut line.ground_property, |id| self.is_property(id));
        self.field(&mut line.ranked_by, |id| self.is_property(id));
        self.field(&mut line.event, |id| self.is_event(id));
        self.formula(&mut line.formula);
        let compared = compared_property(&line.left).map(str::to_owned);
        let compared = compared.as_deref();
        for (key, slot) in SLOT_KEYS.into_iter().zip(slots_mut(line)) {
            let slot_about = match key {
                "right" => compared,
                "value" => about,
                _ => None,
            };
            self.slot(slot, slot_about);
        }
        let branches = std::mem::take(&mut line.branches);
        for (mut state, mut value) in branches {
            if self.is_state(about, &state) && self.hit(&mut state) {
                continue;
            }
            if self.value(&mut value, None) {
                continue;
            }
            line.branches.insert(state, value);
        }
        for summary in &mut line.summaries {
            self.slot(&mut summary.value, None);
        }
        self.values(&mut line.items, None);
        for row in &mut line.rows {
            self.slot(&mut row.value, None);
            self.slot(&mut row.condition, None);
        }
    }

    /// Returns the first state of property `property` that is not the target state.
    fn fallback_state(&self, property: &str) -> Option<String> {
        self.states
            .get(property)?
            .iter()
            .find(|state| !self.is_state(Some(property), state))
            .cloned()
    }

    /// Applies the change to the start of `property`.
    ///
    /// A start keeps a state to fall back on. A cleared state of the property's own, in its base, its otherwise or
    /// the state chip of its value, takes the first state the property still holds. A cleared ground state of a start
    /// on the ground takes the first state the ground property still holds.
    fn property(&self, property: &mut SheetProperty) {
        let own = property.id.clone();
        let own = Some(own.as_str());
        self.slot(&mut property.kept_per_tick, None);
        let Some(starts) = property.starts_as.as_mut() else {
            return;
        };
        let fallback = own.and_then(|own| self.fallback_state(own));
        self.field(&mut starts.property, |id| self.is_property(id));
        match starts.property.clone() {
            Some(on_ground) => {
                if let Some(state) = starts.state.as_mut()
                    && self.is_state(Some(&on_ground), state)
                    && self.hit(state)
                {
                    starts.state = self.fallback_state(&on_ground);
                }
            }
            None => self.field(&mut starts.state, |state| self.is_state(None, state)),
        }
        let value_state = matches!(
            starts.value.value(),
            Some(SheetValue::Chip(SheetChip::State(state))) if self.is_state(own, state)
        );
        self.slot(&mut starts.value, own);
        if value_state
            && starts.value.is_blank()
            && let Some(state) = &fallback
        {
            starts.value = Slot::Filled(SheetValue::Chip(SheetChip::State(state.clone())));
        }
        for field in [&mut starts.base, &mut starts.otherwise] {
            if let Some(state) = field
                && self.is_state(own, state)
                && self.hit(state)
            {
                field.clone_from(&fallback);
            }
        }
        starts.rows.retain_mut(|row| {
            self.slot(&mut row.chance, None);
            match row.state.as_mut() {
                Some(state) if self.is_state(own, state) => !self.hit(state),
                _ => true,
            }
        });
        starts
            .shapes
            .retain_mut(|shape| !(self.is_state(own, &shape.state) && self.hit(&mut shape.state)));
    }

    /// Applies the change to every reference in `document`.
    fn document(&self, document: &mut SheetDocument) {
        for property in all_properties_mut(document) {
            self.property(property);
        }
        if let Some(ground) = document.ground.as_mut() {
            ground.blocked.retain_mut(|blocked| {
                let state_hit = self.is_state(Some(&blocked.property), &blocked.state) && self.hit(&mut blocked.state);
                let property_hit = self.is_property(&blocked.property) && self.hit(&mut blocked.property);
                !(state_hit || property_hit)
            });
        }
        document.measures.retain_mut(|measure| {
            self.slot(&mut measure.value, None);
            let property = measure.property.clone();
            // `|` visits every field, and a rename reaches each of them.
            let cleared = self.names_cleared(&mut measure.over, |id| self.is_thing(id))
                | self.names_cleared(&mut measure.property, |id| self.is_property(id))
                | self.names_cleared(&mut measure.state, |state| self.is_state(property.as_deref(), state))
                | self.names_cleared(&mut measure.event, |id| self.is_event(id));
            !cleared
        });
        let colors = std::mem::take(&mut document.colors);
        for (mut thing, mut rule) in colors {
            if self.is_thing(&thing) && self.hit(&mut thing) {
                continue;
            }
            if let Some(property) = rule.property.as_mut()
                && self.is_property(property)
                && self.hit(property)
            {
                continue;
            }
            let property = rule.property.clone();
            self.field(&mut rule.shade_state, |state| self.is_state(property.as_deref(), state));
            rule.shades
                .retain_mut(|shade| !(self.is_property(&shade.property) && self.hit(&mut shade.property)));
            document.colors.insert(thing, rule);
        }
        for button in &mut document.buttons {
            button.does.retain_mut(|step| {
                let property = step.property.clone();
                self.slot(&mut step.value, property.as_deref());
                step.properties.retain_mut(|id| !(self.is_property(id) && self.hit(id)));
                let cleared = self.names_cleared(&mut step.thing, |id| self.is_thing(id))
                    | self.names_cleared(&mut step.property, |id| self.is_property(id))
                    | self.names_cleared(&mut step.card, |id| self.is_card(id));
                !cleared
            });
        }
        for card in &mut document.cards {
            if self.is_thing(&card.every) {
                let _cleared = self.hit(&mut card.every);
            }
            for line in card.line_tables.values_mut() {
                self.line(line);
            }
        }
    }
}

/// Returns whether `id` has the form of a line id, `l` and a number.
pub(super) fn is_line_id(id: &str) -> bool {
    id.strip_prefix('l')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Returns `formula` with each reference that `replace` maps to new text replaced by it.
///
/// A reference is a word `head` or `head.tail`, as the formula parser reads them, and `replace` takes the head and the
/// tail. Numbers, operators and everything else are copied as they are.
pub(super) fn rewrite_formula(formula: &str, mut replace: impl FnMut(&str, Option<&str>) -> Option<String>) -> String {
    let bytes = formula.as_bytes();
    let mut out = String::with_capacity(formula.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        let start = index;
        if byte.is_ascii_digit() {
            index = digits(bytes, index);
            if bytes.get(index) == Some(&b'.') && bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
                index = digits(bytes, index + 1);
            }
            if bytes.get(index) == Some(&b'e') {
                let sign = usize::from(bytes.get(index + 1) == Some(&b'-'));
                if bytes.get(index + 1 + sign).is_some_and(u8::is_ascii_digit) {
                    index = digits(bytes, index + 1 + sign);
                }
            }
            out.push_str(&formula[start..index]);
        } else if byte.is_ascii_lowercase() {
            index = word(bytes, index);
            let head = &formula[start..index];
            let tail = if bytes.get(index) == Some(&b'.') && bytes.get(index + 1).is_some_and(u8::is_ascii_lowercase) {
                let tail_start = index + 1;
                index = word(bytes, tail_start);
                Some(&formula[tail_start..index])
            } else {
                None
            };
            match replace(head, tail) {
                Some(replacement) => out.push_str(&replacement),
                None => out.push_str(&formula[start..index]),
            }
        } else {
            let length = formula[start..].chars().next().map_or(1, char::len_utf8);
            index += length;
            out.push_str(&formula[start..index]);
        }
    }
    out
}

/// Returns the index past the digits at `index` of `bytes`.
fn digits(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    index
}

/// Returns the index past the word characters, `[a-z0-9_]`, at `index` of `bytes`.
fn word(bytes: &[u8], mut index: usize) -> usize {
    while bytes
        .get(index)
        .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
    {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::rewrite_formula;

    #[test]
    fn a_formula_rewrite_touches_only_the_references_it_maps() {
        let formula = "trail_falloff * l2.amount + my.reward_held - 2.5e-7 * length(l3)";
        let rewritten = rewrite_formula(formula, |head, tail| match (head, tail) {
            ("trail_falloff", None) => Some("fade".to_owned()),
            ("my", Some("reward_held")) => Some("my.reward".to_owned()),
            _ => None,
        });
        assert_eq!(rewritten, "fade * l2.amount + my.reward - 2.5e-7 * length(l3)");
        assert_eq!(rewrite_formula("é + 1", |_, _| None), "é + 1");
    }
}
