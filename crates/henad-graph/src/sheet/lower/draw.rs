//! Lines that draw: a chance and a random number.
//!
//! Each draw takes the key of the line that holds it composed with its slot path, as [`super::keys`] describes. A
//! chance is one `rand.chance`. A chance once for each of a count `n` holds with `1 - (1 - p)^n`, each one counted
//! being a separate try. When `p` is the same for every cell and `n` counts the cells around, the power is a
//! `math.powi` whose `max_exponent` is the number of cells around (8, 4 or 9), and otherwise a `math.pow` behind a
//! `logic.select` that takes 1 for a count of 0, since a power of 0 to the 0 is not 1 on every device. A count of
//! agents is a `math.pow`, never a `math.powi`, whose exponent clamps at
//! [`MAX_POWI_EXPONENT`](crate::limits::MAX_POWI_EXPONENT). A random number between two values is
//! `low + rand.float(high - low)`.

use super::Site;
use super::card::CardLowering;
use super::keys;
use super::look::neighbor_count;
use super::value::{Operand, number, output};
use crate::ids::NodeId;
use crate::sheet::check::Place;
use crate::sheet::document::{LineKind, SheetChip, SheetLine, SheetValue, SummaryKind};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, PortRef};

impl<'a> CardLowering<'_, 'a> {
    /// Writes the draw `op` over `inputs` at `site`, keyed by the line that owns `site` and the site's slot path, and
    /// returns its temporary id.
    pub(super) fn draw(&mut self, site: &Site, op: &str, inputs: Vec<(&str, InputValue)>) -> Result<NodeId, String> {
        let super::ids::Owner::Line { id, .. } = &site.owner else {
            return Err("a draw stands outside a line".to_owned());
        };
        let line = self
            .relations
            .line(id)
            .ok_or_else(|| format!("'{id}' is no line of the card"))?;
        let document = self.builder.document;
        let key = keys::draw_key(keys::owner_key(line.key, &document.sheet.id, id), &site.path);
        self.builder
            .record_draw(key, format!("line {id}"), Place::Line(self.card_index, id.clone()));
        Ok(self.builder.emit(
            site,
            op,
            vec![("key", ConfigValue::Integer(i64::from(key)))],
            inputs
                .into_iter()
                .map(|(port, input)| (port.to_owned(), input))
                .collect(),
        ))
    }

    /// Returns the outcome of a chance, drawn once for each of its count when it has one.
    pub(super) fn chance(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        let chance = self.slot(&site.at("chance"), &line.chance)?;
        let chance_input = self.builder.input_as(site, &chance, &ValueType::F32)?;
        let probability = if line.tries.is_blank() {
            chance_input
        } else {
            let tries_site = site.at("tries");
            let tries = self.slot(&tries_site, &line.tries)?;
            let missed = self
                .builder
                .node_input(site, "math.sub", vec![("a", number(1.0)), ("b", chance_input)]);
            let uniform = chance.is_uniform();
            let missed_every_try = match self.count_bound(line.tries.value()) {
                Some(bound) if uniform => {
                    let exponent = self.builder.input_as(&tries_site, &tries, &ValueType::U32)?;
                    let node = self.builder.emit(
                        site,
                        "math.powi",
                        vec![("max_exponent", ConfigValue::Integer(i64::from(bound)))],
                        vec![("base".to_owned(), missed), ("exponent".to_owned(), exponent)],
                    );
                    output(node)
                }
                _ => {
                    let exponent = self.builder.input_as(&tries_site, &tries, &ValueType::F32)?;
                    let power = self.builder.node_input(
                        site,
                        "math.pow",
                        vec![("base", missed), ("exponent", exponent.clone())],
                    );
                    // No try at all misses every try, whatever the chance.
                    self.one_where_none(site, exponent, power)
                }
            };
            self.builder
                .node_input(site, "math.sub", vec![("a", number(1.0)), ("b", missed_every_try)])
        };
        let node = self.draw(site, "rand.chance", vec![("p", probability)])?;
        Ok(Operand::Link(PortRef { node, port: None }, ValueType::Bool))
    }

    /// Returns the most a count `tries` can be when it counts the cells around: a Count, or the `count` part or a Count
    /// summary of a Look around. `None` for any other value.
    pub(super) fn count_bound(&self, tries: Option<&SheetValue>) -> Option<u32> {
        let (counting, part) = match tries? {
            SheetValue::Chip(SheetChip::Result {
                line,
                part,
                property: None,
            }) => (self.card.line_tables.get(line)?, part.as_deref()),
            SheetValue::Phrase(phrase) => (phrase.as_ref(), None),
            _ => return None,
        };
        let bound = neighbor_count(counting.around?)?;
        match (counting.kind, part) {
            (LineKind::Count, None) | (LineKind::LookAround, Some("count")) => Some(bound),
            (LineKind::LookAround, Some(part)) => counting
                .summaries
                .iter()
                .any(|summary| summary.id == part && summary.summary == Some(SummaryKind::Count))
                .then_some(bound),
            _ => None,
        }
    }

    /// Returns a random number between the line's low and high values.
    pub(super) fn random_number(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        let low = self.slot(&site.at("low"), &line.low)?;
        let high = self.slot(&site.at("high"), &line.high)?;
        let low = self.builder.input_as(site, &low, &ValueType::F32)?;
        let high = self.builder.input_as(site, &high, &ValueType::F32)?;
        let span = self
            .builder
            .node_input(site, "math.sub", vec![("a", high), ("b", low.clone())]);
        let node = self.draw(site, "rand.float", vec![("max", span)])?;
        Ok(self
            .builder
            .node_value(site, "math.add", vec![("a", output(node)), ("b", low)], ValueType::F32))
    }
}
