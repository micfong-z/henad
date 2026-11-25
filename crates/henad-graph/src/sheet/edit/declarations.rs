//! Edit commands on the declarations of a sheet: properties, states, parameters, measures, events, buttons, cards and
//! the ground, and the renames, new ids, recolours and deletes of their items.

use super::references::{Change, Target, change_references};
use super::{
    Draft, IdSpace, STATE_COLORS, SheetEditError, SheetItem, clean_name, mint_id, missing, refuse_infinite,
    refuse_taken,
};
use crate::check::RUNS_CSV_COLUMNS;
use crate::ids::is_id;
use crate::sheet::document::{
    BlockedState, ButtonStep, ButtonStepRule, CardRuns, ColorRule, ColorRuleKind, Marks, MeasureRule, ParameterType,
    PropertyType, SheetButton, SheetCard, SheetChip, SheetEvent, SheetMeasure, SheetParameter, SheetProperty,
    SheetState, SheetValue, Slot,
};
use crate::sheet::limits::MAX_SHEET_ID_BYTES;
use crate::sheet::sentence;
use crate::value::Color;

/// Gives the entry `id` of `entries`, `(id, name)` pairs of one list of `what`, the cleaned name `name`.
///
/// # Errors
///
/// Returns [`SheetEditError::Taken`] when another entry holds the name, case aside, and [`SheetEditError::Missing`]
/// when no entry holds `id`.
fn rename_entry<'a>(
    entries: impl Iterator<Item = (&'a str, &'a mut String)>,
    id: &str,
    name: String,
    what: &str,
) -> Result<(), SheetEditError> {
    let entries: Vec<(&str, &mut String)> = entries.collect();
    let others = entries.iter().filter(|(known, _)| *known != id);
    refuse_taken(&name, others.map(|(_, other)| other.as_str()), what)?;
    let (_, held) = entries
        .into_iter()
        .find(|(known, _)| *known == id)
        .ok_or_else(|| missing(what, id))?;
    *held = name;
    Ok(())
}

/// Returns an error when the card id `id` is longer than [`MAX_SHEET_ID_BYTES`].
fn refuse_long_card_id(id: &str) -> Result<(), SheetEditError> {
    if id.len() > MAX_SHEET_ID_BYTES {
        return Err(SheetEditError::Invalid(format!(
            "the card id '{id}' exceeds {MAX_SHEET_ID_BYTES} bytes"
        )));
    }
    Ok(())
}

impl Draft {
    /// Adds a property named `name` of `property_type` to `thing`.
    pub(super) fn add_property(
        &mut self,
        thing: &str,
        name: &str,
        property_type: PropertyType,
    ) -> Result<Vec<String>, SheetEditError> {
        let name = clean_name(name, "property")?;
        let id = self.mint_item(&name)?;
        let properties = self.thing_properties_mut(thing)?;
        refuse_taken(
            &name,
            properties.iter().map(|property| property.name.as_str()),
            "property",
        )?;
        properties.push(SheetProperty::new(id.clone(), name, property_type));
        if property_type == PropertyType::States {
            self.color_by_state(thing, &id);
        }
        Ok(vec![id])
    }

    /// Colours `thing` by the states of its property `property`, when `thing` has no colour rule.
    fn color_by_state(&mut self, thing: &str, property: &str) {
        if !self.document.colors.contains_key(thing) {
            let mut rule = ColorRule::new(ColorRuleKind::ByState);
            rule.property = Some(property.to_owned());
            self.document.colors.insert(thing.to_owned(), rule);
        }
    }

    /// Clears every reference to the property `id` and to its states.
    fn clear_property_references(&mut self, id: &str) {
        // The states go first, so every chip of a state of the property is cleared with them.
        let states: Vec<String> = self
            .property(id)
            .map(|property| property.states.iter().map(|state| state.id.clone()).collect())
            .unwrap_or_default();
        for state in states {
            change_references(
                &mut self.document,
                &Target::State {
                    property: id.to_owned(),
                    state,
                },
                Change::Clear,
            );
        }
        change_references(&mut self.document, &Target::Property(id.to_owned()), Change::Clear);
    }

    /// Gives the property `id` the type `property_type`, clearing every reference to it and to its states.
    pub(super) fn set_property_type(&mut self, id: &str, property_type: PropertyType) -> Result<(), SheetEditError> {
        let property = self.property(id).ok_or_else(|| missing("property", id))?;
        if property.property_type == property_type {
            return Ok(());
        }
        self.clear_property_references(id);
        // A colour rule over the property no longer fits its type.
        self.document
            .colors
            .retain(|_, rule| rule.property.as_deref() != Some(id));
        let thing = self.thing_of_property(id);
        let property = self.property_mut(id)?;
        property.property_type = property_type;
        property.states.clear();
        property.starts_as = None;
        property.marks = None;
        if property_type == PropertyType::States
            && let Some(thing) = thing
        {
            self.color_by_state(&thing, id);
        }
        Ok(())
    }

    /// Adds a state named `name` to the States property `property`.
    pub(super) fn add_state(
        &mut self,
        property: &str,
        name: &str,
        color: Option<Color>,
    ) -> Result<Vec<String>, SheetEditError> {
        let name = clean_name(name, "state")?;
        let property = self.property_mut(property)?;
        if property.property_type != PropertyType::States {
            return Err(SheetEditError::Invalid(format!(
                "'{}' is not a States property",
                property.id
            )));
        }
        refuse_taken(&name, property.states.iter().map(|state| state.name.as_str()), "state")?;
        let id = mint_id(&name, IdSpace::State, |id| {
            property.states.iter().any(|state| state.id == id)
        })?;
        let color = color.unwrap_or(STATE_COLORS[property.states.len() % STATE_COLORS.len()]);
        property.states.push(SheetState::new(id.clone(), name, color));
        Ok(vec![id])
    }

    /// Adds a parameter named `name` of `parameter_type`, with the range `min` to `max`, the value `default` and
    /// the unit `unit`.
    pub(super) fn add_parameter(
        &mut self,
        name: &str,
        parameter_type: ParameterType,
        min: f64,
        max: f64,
        default: f64,
        unit: Option<String>,
    ) -> Result<Vec<String>, SheetEditError> {
        refuse_infinite(&[min, max, default])?;
        let name = clean_name(name, "parameter")?;
        refuse_taken(
            &name,
            self.document.parameters.iter().map(|parameter| parameter.name.as_str()),
            "parameter",
        )?;
        // A parameter's id names its column in sweep results, and the fixed columns beside it count as taken.
        let taken = self.item_ids();
        let id = mint_id(&name, IdSpace::Item, |id| {
            taken.contains(id) || RUNS_CSV_COLUMNS.contains(&id)
        })?;
        let mut parameter = SheetParameter::new(id.clone(), name, parameter_type, min, max, default);
        parameter.unit = unit.filter(|unit| !unit.trim().is_empty());
        self.document.parameters.push(parameter);
        Ok(vec![id])
    }

    /// Adds an event named `name`.
    pub(super) fn add_event(&mut self, name: &str) -> Result<Vec<String>, SheetEditError> {
        let name = clean_name(name, "event")?;
        refuse_taken(
            &name,
            self.document.events.iter().map(|event| event.name.as_str()),
            "event",
        )?;
        let id = self.mint_item(&name)?;
        self.document.events.push(SheetEvent::new(id.clone(), name));
        Ok(vec![id])
    }

    /// Adds a button named `name` that does `rule` to `thing`.
    pub(super) fn add_button(
        &mut self,
        name: &str,
        rule: ButtonStepRule,
        thing: &str,
    ) -> Result<Vec<String>, SheetEditError> {
        let name = clean_name(name, "button")?;
        self.require_thing(thing)?;
        refuse_taken(
            &name,
            self.document.buttons.iter().map(|button| button.name.as_str()),
            "button",
        )?;
        let document = &self.document;
        let id = mint_id(&name, IdSpace::Button, |id| {
            document.buttons.iter().any(|button| button.id == id)
                || rule == ButtonStepRule::RunOnce && document.cards.iter().any(|card| card.id == id)
        })?;
        let mut button = SheetButton::new(id.clone(), name);
        let mut step = ButtonStep::new(rule);
        match rule {
            ButtonStepRule::StartAgain => step.thing = Some(thing.to_owned()),
            ButtonStepRule::RunOnce => {
                step.card = Some(id.clone());
                self.document
                    .cards
                    .push(SheetCard::new(id.clone(), thing, CardRuns::Once));
            }
            _ => {
                return Err(SheetEditError::Invalid(
                    "a new button must start again or run once".to_owned(),
                ));
            }
        }
        button.does.push(step);
        self.document.buttons.push(button);
        Ok(vec![id])
    }

    /// Replaces the steps of the button `button` with `does`.
    pub(super) fn set_button_steps(&mut self, button: &str, does: Vec<ButtonStep>) -> Result<(), SheetEditError> {
        for step in &does {
            if let Some(card) = &step.card
                && !self.document.cards.iter().any(|known| known.id == *card)
            {
                return Err(missing("card", card));
            }
        }
        self.document
            .buttons
            .iter_mut()
            .find(|known| known.id == button)
            .ok_or_else(|| missing("button", button))?
            .does = does;
        Ok(())
    }

    /// Returns the id of the card the kind of thing `every` runs each tick.
    fn tick_card_id(every: &str) -> String {
        match every {
            "cells" => "cell_tick".to_owned(),
            _ => format!("{every}_tick"),
        }
    }

    /// Adds the card `every` runs each tick, before every card that runs once.
    pub(super) fn add_card(&mut self, every: &str) -> Result<Vec<String>, SheetEditError> {
        if every == "ground" {
            return Err(SheetEditError::Invalid("the ground cannot have a card".to_owned()));
        }
        self.require_thing(every)?;
        let id = Self::tick_card_id(every);
        refuse_long_card_id(&id)?;
        if self.document.cards.iter().any(|card| card.id == id) {
            return Err(SheetEditError::Taken(format!("the sheet already has the card '{id}'")));
        }
        let at = self
            .document
            .cards
            .iter()
            .position(|card| card.runs == CardRuns::Once)
            .unwrap_or(self.document.cards.len());
        self.document
            .cards
            .insert(at, SheetCard::new(id.clone(), every, CardRuns::EachTick));
        Ok(vec![id])
    }

    /// Renames each average of property `id` that holds its automatic name, the words of
    /// [`sentence::average_measure`] for `old`, the property's name before a rename to `new`.
    ///
    /// A measure named by hand keeps its name, and so does a follower whose new name another measure holds, case
    /// aside.
    fn follow_property_name(&mut self, id: &str, old: &str, new: &str) {
        let (before, after) = (sentence::average_measure(old), sentence::average_measure(new));
        let follows = |measure: &SheetMeasure| {
            measure.rule == MeasureRule::Average
                && measure.name == before
                && matches!(measure.value.value(), Some(SheetValue::Chip(SheetChip::My(property))) if property == id)
        };
        let taken = self
            .document
            .measures
            .iter()
            .any(|measure| !follows(measure) && measure.name.to_lowercase() == after.to_lowercase());
        if taken {
            return;
        }
        for measure in &mut self.document.measures {
            if follows(measure) {
                measure.name.clone_from(&after);
            }
        }
    }

    /// Renames `item` to `name`.
    pub(super) fn rename(&mut self, item: &SheetItem, name: &str) -> Result<(), SheetEditError> {
        match item {
            SheetItem::Property(id) => {
                let name = clean_name(name, "property")?;
                let siblings: Vec<String> = self
                    .thing_of_property(id)
                    .and_then(|thing| self.properties_of(&thing))
                    .into_iter()
                    .flatten()
                    .filter(|property| property.id != *id)
                    .map(|property| property.name.clone())
                    .collect();
                refuse_taken(&name, siblings.iter().map(String::as_str), "property")?;
                let old = std::mem::replace(&mut self.property_mut(id)?.name, name.clone());
                self.follow_property_name(id, &old, &name);
            }
            SheetItem::State { property, state } => {
                let name = clean_name(name, "state")?;
                let states = &mut self.property_mut(property)?.states;
                rename_entry(
                    states.iter_mut().map(|known| (known.id.as_str(), &mut known.name)),
                    state,
                    name,
                    "state",
                )?;
            }
            SheetItem::Parameter(id) => {
                let name = clean_name(name, "parameter")?;
                let parameters = self.document.parameters.iter_mut();
                rename_entry(
                    parameters.map(|known| (known.id.as_str(), &mut known.name)),
                    id,
                    name,
                    "parameter",
                )?;
            }
            SheetItem::Measure(id) => {
                let name = clean_name(name, "measure")?;
                let measures = self.document.measures.iter_mut();
                rename_entry(
                    measures.map(|known| (known.id.as_str(), &mut known.name)),
                    id,
                    name,
                    "measure",
                )?;
            }
            SheetItem::Event(id) => {
                let name = clean_name(name, "event")?;
                let events = self.document.events.iter_mut();
                rename_entry(
                    events.map(|known| (known.id.as_str(), &mut known.name)),
                    id,
                    name,
                    "event",
                )?;
            }
            SheetItem::Button(id) => {
                let name = clean_name(name, "button")?;
                let buttons = self.document.buttons.iter_mut();
                rename_entry(
                    buttons.map(|known| (known.id.as_str(), &mut known.name)),
                    id,
                    name,
                    "button",
                )?;
            }
            SheetItem::AgentKind(id) => {
                let name = clean_name(name, "kind of agent")?;
                self.agent_kind_mut(id)?.name = name;
            }
            SheetItem::Card(id) => {
                return Err(SheetEditError::Invalid(format!("card '{id}' has no name of its own")));
            }
        }
        Ok(())
    }

    /// Returns the kind of thing that holds property `id`.
    fn thing_of_property(&self, id: &str) -> Option<String> {
        let has = |properties: &[SheetProperty]| properties.iter().any(|property| property.id == id);
        if self.document.cells.as_deref().is_some_and(has) {
            return Some("cells".to_owned());
        }
        if let Some(kind) = self.document.agents.iter().find(|kind| has(&kind.properties)) {
            return Some(kind.id.clone());
        }
        self.document
            .ground
            .as_ref()
            .filter(|ground| has(&ground.properties))
            .map(|_| "ground".to_owned())
    }

    /// Returns the properties of the kind of thing `thing`.
    fn properties_of(&self, thing: &str) -> Option<&[SheetProperty]> {
        match thing {
            "cells" => self.document.cells.as_deref(),
            "ground" => self.document.ground.as_ref().map(|ground| ground.properties.as_slice()),
            _ => self
                .document
                .agents
                .iter()
                .find(|kind| kind.id == thing)
                .map(|kind| kind.properties.as_slice()),
        }
    }

    /// Returns the property `id` of the ground, mutable, and an error naming `what` when the ground holds no such
    /// property or it is no property of type `property_type`.
    fn ground_property_mut(
        &mut self,
        id: &str,
        property_type: PropertyType,
        what: &str,
    ) -> Result<&mut SheetProperty, SheetEditError> {
        let property = self
            .document
            .ground
            .as_mut()
            .and_then(|ground| ground.properties.iter_mut().find(|property| property.id == id))
            .ok_or_else(|| missing("ground property", id))?;
        if property.property_type != property_type {
            return Err(SheetEditError::Invalid(format!("'{id}' is not a {what} of the ground")));
        }
        Ok(property)
    }

    /// Sets the marks rule and the share kept per tick of the Number property `id` of the ground. No marks rule clears
    /// the share kept.
    pub(super) fn set_marks(
        &mut self,
        id: &str,
        marks: Option<Marks>,
        kept_per_tick: Slot,
    ) -> Result<(), SheetEditError> {
        let property = self.ground_property_mut(id, PropertyType::Number, "Number property")?;
        property.marks = marks;
        property.kept_per_tick = if marks.is_some() { kept_per_tick } else { Slot::Blank };
        Ok(())
    }

    /// Keeps agents out of the cells whose ground property `property` is `state`, or lets them in.
    pub(super) fn set_blocked(&mut self, property: &str, state: &str, blocked: bool) -> Result<(), SheetEditError> {
        let holder = self.ground_property_mut(property, PropertyType::States, "States property")?;
        if !holder.states.iter().any(|known| known.id == state) {
            return Err(missing("state", state));
        }
        let ground = self.document.ground.get_or_insert_with(Default::default);
        let held = ground
            .blocked
            .iter()
            .any(|known| known.property == property && known.state == state);
        if blocked && !held {
            ground.blocked.push(BlockedState::new(property, state));
        } else if !blocked {
            ground
                .blocked
                .retain(|known| !(known.property == property && known.state == state));
        }
        Ok(())
    }

    /// Returns an error unless the sheet holds `item` under its kind.
    ///
    /// Note that an id held by another kind of item counts as missing, as a property's id given as a parameter's.
    fn require_item(&self, item: &SheetItem) -> Result<(), SheetEditError> {
        let document = &self.document;
        let (what, id, held) = match item {
            SheetItem::Property(id) => ("property", id, self.property(id).is_some()),
            SheetItem::State { property, state } => {
                let holder = self.property(property).ok_or_else(|| missing("property", property))?;
                ("state", state, holder.states.iter().any(|known| known.id == *state))
            }
            SheetItem::Parameter(id) => ("parameter", id, document.parameters.iter().any(|known| known.id == *id)),
            SheetItem::Measure(id) => ("measure", id, document.measures.iter().any(|known| known.id == *id)),
            SheetItem::Event(id) => ("event", id, document.events.iter().any(|known| known.id == *id)),
            SheetItem::Button(id) => ("button", id, document.buttons.iter().any(|known| known.id == *id)),
            SheetItem::Card(id) => ("card", id, document.cards.iter().any(|known| known.id == *id)),
            SheetItem::AgentKind(id) => ("kind of agent", id, document.agents.iter().any(|known| known.id == *id)),
        };
        if held { Ok(()) } else { Err(missing(what, id)) }
    }

    /// Gives `item` the id `id` and rewrites every reference to it.
    #[expect(clippy::too_many_lines, reason = "one arm per kind of item reads best as one table")]
    pub(super) fn change_id(&mut self, item: &SheetItem, id: &str) -> Result<(), SheetEditError> {
        if !is_id(id) || id.len() > MAX_SHEET_ID_BYTES {
            return Err(SheetEditError::Invalid(format!(
                "invalid sheet id '{id}', expected lower case letters, digits and single '_', starting with a \
                 letter, at most {MAX_SHEET_ID_BYTES} bytes"
            )));
        }
        let space = match item {
            SheetItem::Property(_) | SheetItem::Parameter(_) | SheetItem::Measure(_) | SheetItem::Event(_) => {
                IdSpace::Item
            }
            SheetItem::State { .. } => IdSpace::State,
            SheetItem::Button(_) => IdSpace::Button,
            SheetItem::Card(_) => IdSpace::Card,
            SheetItem::AgentKind(_) => IdSpace::Thing,
        };
        if space.is_reserved(id) || matches!(item, SheetItem::Parameter(_)) && RUNS_CSV_COLUMNS.contains(&id) {
            return Err(SheetEditError::Reserved(id.to_owned()));
        }
        self.require_item(item)?;
        let taken = |held: bool| {
            if held {
                Err(SheetEditError::Taken(format!("the id '{id}' is already in use")))
            } else {
                Ok(())
            }
        };
        match item {
            SheetItem::Property(old) | SheetItem::Parameter(old) | SheetItem::Measure(old) | SheetItem::Event(old) => {
                if old == id {
                    return Ok(());
                }
                taken(self.item_ids().contains(id))?;
                match item {
                    SheetItem::Property(_) => {
                        change_references(&mut self.document, &Target::Property(old.clone()), Change::Rename(id));
                        self.property_mut(old)?.id = id.to_owned();
                    }
                    SheetItem::Parameter(_) => {
                        change_references(&mut self.document, &Target::Parameter(old.clone()), Change::Rename(id));
                        if let Some(parameter) = self.document.parameters.iter_mut().find(|known| known.id == *old) {
                            id.clone_into(&mut parameter.id);
                        }
                    }
                    SheetItem::Event(_) => {
                        change_references(&mut self.document, &Target::Event(old.clone()), Change::Rename(id));
                        if let Some(event) = self.document.events.iter_mut().find(|known| known.id == *old) {
                            id.clone_into(&mut event.id);
                        }
                    }
                    _ => {
                        let measure = self
                            .document
                            .measures
                            .iter_mut()
                            .find(|known| known.id == *old)
                            .ok_or_else(|| missing("measure", old))?;
                        id.clone_into(&mut measure.id);
                    }
                }
            }
            SheetItem::State { property, state } => {
                if state == id {
                    return Ok(());
                }
                let holder = self.property(property).ok_or_else(|| missing("property", property))?;
                taken(holder.states.iter().any(|known| known.id == id))?;
                change_references(
                    &mut self.document,
                    &Target::State {
                        property: property.clone(),
                        state: state.clone(),
                    },
                    Change::Rename(id),
                );
                if let Some(known) = self
                    .property_mut(property)?
                    .states
                    .iter_mut()
                    .find(|known| known.id == *state)
                {
                    id.clone_into(&mut known.id);
                }
            }
            SheetItem::Button(old) => {
                if old == id {
                    return Ok(());
                }
                taken(self.document.buttons.iter().any(|button| button.id == id))?;
                let card_follows = self.document.cards.iter().any(|card| card.id == *old)
                    && self.document.buttons.iter().any(|button| {
                        button.id == *old
                            && button
                                .does
                                .iter()
                                .any(|step| step.card.as_deref() == Some(old.as_str()))
                    });
                if card_follows {
                    taken(self.document.cards.iter().any(|card| card.id == id))?;
                    self.rename_card(old, id);
                }
                if let Some(button) = self.document.buttons.iter_mut().find(|button| button.id == *old) {
                    id.clone_into(&mut button.id);
                }
            }
            SheetItem::Card(old) => {
                if old == id {
                    return Ok(());
                }
                taken(self.document.cards.iter().any(|card| card.id == id))?;
                self.rename_card(old, id);
            }
            SheetItem::AgentKind(old) => {
                if old == id {
                    return Ok(());
                }
                taken(self.document.agents.iter().any(|kind| kind.id == id))?;
                let old_tick = Self::tick_card_id(old);
                let new_tick = Self::tick_card_id(id);
                if self.document.cards.iter().any(|card| card.id == old_tick) {
                    refuse_long_card_id(&new_tick)?;
                    taken(self.document.cards.iter().any(|card| card.id == new_tick))?;
                    self.rename_card(&old_tick, &new_tick);
                }
                change_references(&mut self.document, &Target::Thing(old.clone()), Change::Rename(id));
                self.agent_kind_mut(old)?.id = id.to_owned();
            }
        }
        Ok(())
    }

    /// Gives card `old` the id `new`, and every button step that runs it.
    fn rename_card(&mut self, old: &str, new: &str) {
        change_references(&mut self.document, &Target::Card(old.to_owned()), Change::Rename(new));
        if let Some(card) = self.document.cards.iter_mut().find(|card| card.id == old) {
            new.clone_into(&mut card.id);
        }
    }

    /// Recolours the state or measure `item`.
    pub(super) fn recolor(&mut self, item: &SheetItem, color: Color) -> Result<(), SheetEditError> {
        match item {
            SheetItem::State { property, state } => {
                let holder = self.property_mut(property)?;
                let known = holder
                    .states
                    .iter_mut()
                    .find(|known| known.id == *state)
                    .ok_or_else(|| missing("state", state))?;
                let old = known.color;
                known.color = color;
                for measure in &mut self.document.measures {
                    if measure.rule == MeasureRule::CountInState
                        && measure.property.as_deref() == Some(property.as_str())
                        && measure.state.as_deref() == Some(state.as_str())
                        && measure.color == old
                    {
                        measure.color = color;
                    }
                }
            }
            SheetItem::Measure(id) => {
                self.document
                    .measures
                    .iter_mut()
                    .find(|known| known.id == *id)
                    .ok_or_else(|| missing("measure", id))?
                    .color = color;
            }
            _ => {
                return Err(SheetEditError::Invalid(
                    "only a state or a measure has a colour".to_owned(),
                ));
            }
        }
        Ok(())
    }

    /// Deletes `item` and clears every reference to it.
    pub(super) fn delete(&mut self, item: &SheetItem) -> Result<(), SheetEditError> {
        match item {
            SheetItem::Property(id) => {
                self.property(id).ok_or_else(|| missing("property", id))?;
                self.clear_property_references(id);
                if let Some(cells) = self.document.cells.as_mut() {
                    cells.retain(|property| property.id != *id);
                }
                for kind in &mut self.document.agents {
                    kind.properties.retain(|property| property.id != *id);
                }
                if let Some(ground) = self.document.ground.as_mut() {
                    ground.properties.retain(|property| property.id != *id);
                }
            }
            SheetItem::State { property, state } => {
                let holder = self.property(property).ok_or_else(|| missing("property", property))?;
                if !holder.states.iter().any(|known| known.id == *state) {
                    return Err(missing("state", state));
                }
                change_references(
                    &mut self.document,
                    &Target::State {
                        property: property.clone(),
                        state: state.clone(),
                    },
                    Change::Clear,
                );
                self.property_mut(property)?.states.retain(|known| known.id != *state);
            }
            SheetItem::Parameter(id) => {
                if !self.document.parameters.iter().any(|known| known.id == *id) {
                    return Err(missing("parameter", id));
                }
                change_references(&mut self.document, &Target::Parameter(id.clone()), Change::Clear);
                self.document.parameters.retain(|known| known.id != *id);
            }
            SheetItem::Measure(id) => {
                if !self.document.measures.iter().any(|known| known.id == *id) {
                    return Err(missing("measure", id));
                }
                self.document.measures.retain(|known| known.id != *id);
            }
            SheetItem::Event(id) => {
                if !self.document.events.iter().any(|known| known.id == *id) {
                    return Err(missing("event", id));
                }
                change_references(&mut self.document, &Target::Event(id.clone()), Change::Clear);
                self.document.events.retain(|known| known.id != *id);
            }
            SheetItem::Button(id) => {
                let button = self
                    .document
                    .buttons
                    .iter()
                    .find(|known| known.id == *id)
                    .ok_or_else(|| missing("button", id))?;
                let cards: Vec<String> = button.does.iter().filter_map(|step| step.card.clone()).collect();
                self.document.buttons.retain(|known| known.id != *id);
                for card in cards {
                    let still_run = self.document.buttons.iter().any(|other| {
                        other
                            .does
                            .iter()
                            .any(|step| step.card.as_deref() == Some(card.as_str()))
                    });
                    let runs_once = self
                        .document
                        .cards
                        .iter()
                        .any(|known| known.id == card && known.runs == CardRuns::Once);
                    if !still_run && runs_once {
                        self.delete_card(&card)?;
                    }
                }
            }
            SheetItem::Card(id) => self.delete_card(id)?,
            SheetItem::AgentKind(id) => {
                return Err(SheetEditError::Invalid(format!(
                    "the kind of agent '{id}' cannot be removed, as it is the world's only kind of agent"
                )));
            }
        }
        Ok(())
    }

    /// Deletes card `id` with its lines, and clears every reference to them and to it.
    fn delete_card(&mut self, id: &str) -> Result<(), SheetEditError> {
        let index = self
            .document
            .cards
            .iter()
            .position(|card| card.id == id)
            .ok_or_else(|| missing("card", id))?;
        let card = self.document.cards.remove(index);
        for line in card.line_tables.keys() {
            change_references(&mut self.document, &Target::Line(line.clone()), Change::Clear);
        }
        change_references(&mut self.document, &Target::Card(id.to_owned()), Change::Clear);
        Ok(())
    }
}
