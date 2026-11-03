//! Edits of the declaration tables: lanes, variants, parameters, statistics, actions and passes.
//!
//! A *reference* is a place that names a declaration: a node's `config.lane`, a literal `{ cat = "health.I" }`, a
//! parameter binding, a pass's `action`, an action's `runs`. A rename rewrites each one, and a delete with any is
//! refused with their list.

use std::collections::BTreeSet;

use henad_core::params::{ParamApply, ParamDescriptor, ParamFormat};

use super::{EditCommand, EditError, GraphBuilder, Scope};
use crate::document::{PassEnabled, PassStage};
use crate::ids::{NodeId, is_id, is_port_segment};
use crate::node::Node;
use crate::value::{ConfigValue, InputValue, Literal, MemberName};

/// Returns the place of `node` in `scope` as a reference names it, such as `node n5 in pass 'seed'`.
pub(super) fn node_place(scope: &Scope, node: NodeId) -> String {
    format!("node {node} in {scope}")
}

/// Returns `descriptor` under the id `id`, its label, kind, apply and format kept.
fn renamed_param(descriptor: &ParamDescriptor, id: &str) -> ParamDescriptor {
    let mut renamed = ParamDescriptor::new(id.to_owned(), descriptor.label().to_owned(), descriptor.kind().clone());
    if descriptor.apply() == ParamApply::OnReload {
        renamed = renamed.on_reload();
    }
    if descriptor.format() == ParamFormat::Percent {
        renamed = renamed.percent();
    }
    renamed
}

/// Returns whether `value`, a configuration value at any depth, holds the one-key table `{ tag = "text" }`.
fn holds_tagged(value: &ConfigValue, tag: &str, text: &str) -> bool {
    match value {
        ConfigValue::Array(items) => items.iter().any(|item| holds_tagged(item, tag, text)),
        ConfigValue::Table(table) => {
            table.get(tag).and_then(ConfigValue::as_str) == Some(text)
                || table.values().any(|item| holds_tagged(item, tag, text))
        }
        _ => false,
    }
}

/// Rewrites every one-key table `{ tag = "from" }` inside `value` to `{ tag = "to" }`.
fn retag(value: &mut ConfigValue, tag: &str, from: &str, to: &str) {
    match value {
        ConfigValue::Array(items) => {
            for item in items {
                retag(item, tag, from, to);
            }
        }
        ConfigValue::Table(table) => {
            if let Some(ConfigValue::String(text)) = table.get_mut(tag)
                && text == from
            {
                *text = to.to_owned();
            }
            for item in table.values_mut() {
                retag(item, tag, from, to);
            }
        }
        _ => {}
    }
}

/// Returns whether `literal` names a member of `owner`, the member `member` when it is set: a variant of a category
/// when `category` holds, and an option of a choice parameter otherwise.
fn names_member(literal: &Literal, owner: &str, member: Option<&str>, category: bool) -> bool {
    let ((Literal::Category(name), true) | (Literal::ChoiceOption(name), false)) = (literal, category) else {
        return false;
    };
    name.owner == owner && member.is_none_or(|member| name.member == member)
}

impl GraphBuilder {
    /// Returns every node of the document with its scope.
    pub(super) fn scoped_nodes(&self) -> Vec<(Scope, &Node)> {
        let passes = self
            .document
            .passes
            .iter()
            .flat_map(|pass| pass.body.nodes.iter().map(|node| (Scope::Pass(pass.id.clone()), node)));
        let functions = self.document.functions.iter().flat_map(|function| {
            function
                .body
                .nodes
                .iter()
                .map(|node| (Scope::Function(function.id.clone()), node))
        });
        passes.chain(functions).collect()
    }

    /// Runs `change` on every node of the document.
    pub(super) fn for_each_node(&mut self, mut change: impl FnMut(&mut Node)) {
        let passes = self.document.passes.iter_mut().map(|pass| &mut pass.body);
        let functions = self.document.functions.iter_mut().map(|function| &mut function.body);
        for body in passes.chain(functions) {
            for node in &mut body.nodes {
                change(node);
            }
        }
    }

    /// Returns an error unless `id` follows the id grammar and no other declaration of `kind` holds it.
    pub(super) fn check_new_id<'a>(
        kind: &str,
        id: &str,
        mut taken: impl Iterator<Item = &'a str>,
    ) -> Result<(), EditError> {
        if !is_id(id) {
            return Err(EditError::Invalid(format!(
                "'{id}' is no {kind} id, which starts with a lowercase letter and holds lowercase letters, digits and `_`"
            )));
        }
        if taken.any(|known| known == id) {
            return Err(EditError::Invalid(format!(
                "the file already declares the {kind} '{id}'"
            )));
        }
        Ok(())
    }

    /// Ids lanes, fields and parameters hold together.
    fn value_ids(&self) -> impl Iterator<Item = &str> {
        self.document
            .lanes
            .iter()
            .map(|lane| lane.id.as_str())
            .chain(self.document.fields.iter().map(|field| field.id.as_str()))
            .chain(self.document.params.iter().map(ParamDescriptor::id))
    }

    /// Ids categories and palettes hold together.
    fn palette_ids(&self) -> impl Iterator<Item = &str> {
        self.document
            .categories
            .iter()
            .map(|category| category.id.as_str())
            .chain(self.document.palettes.iter().map(|palette| palette.id.as_str()))
    }

    /// Returns an error listing `references` when there is any.
    pub(super) fn refuse_referenced(subject: String, references: Vec<String>) -> Result<(), EditError> {
        if references.is_empty() {
            Ok(())
        } else {
            Err(EditError::Referenced { subject, references })
        }
    }

    /// Runs a declaration command.
    #[expect(clippy::too_many_lines, reason = "one arm per command reads best as one table")]
    pub(super) fn run_declaration(&mut self, command: EditCommand) -> Result<(), EditError> {
        match command {
            EditCommand::AddCategory(category) => {
                Self::check_new_id("category", &category.id, self.palette_ids())?;
                let mut names = BTreeSet::new();
                for variant in &category.variants {
                    if !is_port_segment(&variant.name) || !names.insert(variant.name.as_str()) {
                        return Err(EditError::Invalid(format!(
                            "the variant '{}' is no name, or comes twice",
                            variant.name
                        )));
                    }
                }
                self.document.categories.push(category);
                Ok(())
            }
            EditCommand::AddPalette(palette) => {
                Self::check_new_id("palette", &palette.id, self.palette_ids())?;
                self.document.palettes.push(palette);
                Ok(())
            }
            EditCommand::AddLane(lane) => {
                Self::check_new_id("lane", &lane.id, self.value_ids())?;
                self.document.lanes.push(lane);
                Ok(())
            }
            EditCommand::RenameLane { from, to } => self.rename_lane(&from, &to),
            EditCommand::RemoveLane(id) => {
                let position = self.lane_position(&id)?;
                let references = self.lane_references(&id);
                Self::refuse_referenced(format!("the lane '{id}'"), references)?;
                self.document.lanes.remove(position);
                Ok(())
            }
            EditCommand::AddVariant { category, variant } => {
                self.name_bare_counter_links(&category);
                let category = self.category_mut(&category)?;
                if !is_port_segment(&variant.name) {
                    return Err(EditError::Invalid(format!("'{}' is no variant name", variant.name)));
                }
                if category.variants.iter().any(|known| known.name == variant.name) {
                    return Err(EditError::Invalid(format!(
                        "the category '{}' already holds the variant '{}'",
                        category.id, variant.name
                    )));
                }
                category.variants.push(variant);
                Ok(())
            }
            EditCommand::RenameVariant { category, from, to } => self.rename_variant(&category, &from, &to),
            EditCommand::RemoveVariant { category, name } => {
                let position = self.variant_position(&category, &name)?;
                self.name_bare_counter_links(&category);
                let references = self.variant_references(&category, &name);
                Self::refuse_referenced(format!("the variant '{category}.{name}'"), references)?;
                self.category_mut(&category)?.variants.remove(position);
                Ok(())
            }
            EditCommand::AddParam(descriptor) => {
                Self::check_new_id("parameter", descriptor.id(), self.value_ids())?;
                self.document.params.push(descriptor);
                Ok(())
            }
            EditCommand::RenameParam { from, to } => self.rename_param(&from, &to),
            EditCommand::RemoveParam(id) => {
                let position = self.param_position(&id)?;
                let references = self.param_references(&id);
                Self::refuse_referenced(format!("the parameter '{id}'"), references)?;
                self.document.params.remove(position);
                Ok(())
            }
            EditCommand::AddStat(stat) => {
                self.check_new_label(&stat.label)?;
                self.document.stats.push(stat);
                Ok(())
            }
            EditCommand::RenameStat { from, to } => {
                let position = self.stat_position(&from)?;
                self.check_new_label(&to)?;
                self.document.stats[position].label = to;
                Ok(())
            }
            EditCommand::RemoveStat(label) => {
                let position = self.stat_position(&label)?;
                self.document.stats.remove(position);
                Ok(())
            }
            EditCommand::AddAction(action) => {
                Self::check_new_id(
                    "action",
                    &action.id,
                    self.document.actions.iter().map(|known| known.id.as_str()),
                )?;
                self.document.actions.push(action);
                Ok(())
            }
            EditCommand::RenameAction { from, to } => {
                let position = self.action_position(&from)?;
                Self::check_new_id(
                    "action",
                    &to,
                    self.document.actions.iter().map(|known| known.id.as_str()),
                )?;
                self.document.actions[position].id.clone_from(&to);
                for pass in &mut self.document.passes {
                    if pass.action.as_deref() == Some(from.as_str()) {
                        pass.action = Some(to.clone());
                    }
                }
                Ok(())
            }
            EditCommand::RemoveAction(id) => {
                let position = self.action_position(&id)?;
                let references = self
                    .document
                    .passes
                    .iter()
                    .filter(|pass| pass.action.as_deref() == Some(id.as_str()))
                    .map(|pass| format!("the pass '{}'", pass.id))
                    .collect();
                Self::refuse_referenced(format!("the action '{id}'"), references)?;
                self.document.actions.remove(position);
                Ok(())
            }
            EditCommand::AddPass(pass) => {
                Self::check_new_id(
                    "pass",
                    &pass.id,
                    self.document.passes.iter().map(|known| known.id.as_str()),
                )?;
                if !pass.body.nodes.is_empty() {
                    return Err(EditError::Invalid(
                        "a new pass starts empty, and its nodes come by AddNode".to_owned(),
                    ));
                }
                self.insert_pass(pass);
                Ok(())
            }
            EditCommand::RenamePass { from, to } => self.rename_pass(&from, &to),
            EditCommand::RemovePass(id) => self.remove_pass(&id),
            EditCommand::MovePass { id, index } => self.move_pass(&id, index),
            EditCommand::SetPassEnabled { id, enabled } => {
                if let PassEnabled::Param(param) = &enabled {
                    self.param_position(param)?;
                }
                let pass = self
                    .document
                    .passes
                    .iter_mut()
                    .find(|pass| pass.id == id)
                    .ok_or_else(|| EditError::Missing(format!("the file declares no pass '{id}'")))?;
                pass.enabled = enabled;
                Ok(())
            }
            EditCommand::SetParam(descriptor) => {
                let position = self.param_position(descriptor.id())?;
                self.document.params[position] = descriptor;
                Ok(())
            }
            EditCommand::SetWorld(world) => {
                if world.topology() != self.document.world.topology() {
                    return Err(EditError::Invalid(format!(
                        "a {} graph keeps its topology, and the new world is {}",
                        self.document.world.topology().as_str(),
                        world.topology().as_str()
                    )));
                }
                self.document.world = world;
                Ok(())
            }
            other => Err(EditError::Invalid(format!("{other:?} is no declaration edit"))),
        }
    }

    /// Returns the position of the lane `id`.
    fn lane_position(&self, id: &str) -> Result<usize, EditError> {
        self.document
            .lanes
            .iter()
            .position(|lane| lane.id == id)
            .ok_or_else(|| EditError::Missing(format!("the file declares no lane '{id}'")))
    }

    /// Returns each place that names the lane `id`.
    fn lane_references(&self, id: &str) -> Vec<String> {
        self.scoped_nodes()
            .into_iter()
            .filter(|(_, node)| node.config.get("lane").and_then(ConfigValue::as_str) == Some(id))
            .map(|(scope, node)| node_place(&scope, node.id))
            .collect()
    }

    /// Renames the lane `from` to `to` and every node's `config.lane` that names it.
    fn rename_lane(&mut self, from: &str, to: &str) -> Result<(), EditError> {
        let position = self.lane_position(from)?;
        Self::check_new_id("lane", to, self.value_ids().filter(|known| *known != from))?;
        self.document.lanes[position].id = to.to_owned();
        self.for_each_node(|node| {
            if let Some(ConfigValue::String(lane)) = node.config.get_mut("lane")
                && lane == from
            {
                *lane = to.to_owned();
            }
        });
        Ok(())
    }

    /// Returns the category `id` for writing.
    fn category_mut(&mut self, id: &str) -> Result<&mut crate::document::Category, EditError> {
        self.document
            .categories
            .iter_mut()
            .find(|category| category.id == id)
            .ok_or_else(|| EditError::Missing(format!("the file declares no category '{id}'")))
    }

    /// Returns the position of the variant `name` of `category`.
    fn variant_position(&self, category: &str, name: &str) -> Result<usize, EditError> {
        self.document
            .categories
            .iter()
            .find(|known| known.id == category)
            .and_then(|known| known.variants.iter().position(|variant| variant.name == name))
            .ok_or_else(|| EditError::Missing(format!("the file declares no variant '{category}.{name}'")))
    }

    /// Returns the ids of the `stat.count_by` nodes over `category` in each scope.
    fn variant_counters(&self, category: &str) -> BTreeSet<(Scope, NodeId)> {
        self.scoped_nodes()
            .into_iter()
            .filter(|(_, node)| {
                node.op == "stat.count_by"
                    && node.config.get("category").and_then(ConfigValue::as_str) == Some(category)
            })
            .map(|(scope, node)| (scope, node.id))
            .collect()
    }

    /// Names the variant on every bare link to a `stat.count_by` over `category`, when the category holds one variant.
    ///
    /// A bare link names a node's only output, and the canonical text writes a link to a counter of one variant bare.
    /// Called before a variant is added or removed. Otherwise the link would name an output that is no longer the only
    /// one, or hide a reference to the variant removed.
    fn name_bare_counter_links(&mut self, category: &str) {
        let Some(only) = self
            .document
            .categories
            .iter()
            .find(|known| known.id == category)
            .and_then(|known| match known.variants.as_slice() {
                [only] => Some(only.name.clone()),
                _ => None,
            })
        else {
            return;
        };
        let counters = self.variant_counters(category);
        for stat in &mut self.document.stats {
            if stat.from.port.is_none() && counters.iter().any(|(_, node)| *node == stat.from.node) {
                stat.from.port = Some(only.clone());
            }
        }
        let passes = self
            .document
            .passes
            .iter_mut()
            .map(|pass| (Scope::Pass(pass.id.clone()), &mut pass.body));
        let functions = self
            .document
            .functions
            .iter_mut()
            .map(|function| (Scope::Function(function.id.clone()), &mut function.body));
        for (scope, body) in passes.chain(functions) {
            for node in &mut body.nodes {
                for value in node.inputs.values_mut() {
                    if let InputValue::Link(link) = value
                        && link.port.is_none()
                        && counters.contains(&(scope.clone(), link.node))
                    {
                        link.port = Some(only.clone());
                    }
                }
            }
        }
    }

    /// Returns each place that names the variant `name` of `category`.
    fn variant_references(&self, category: &str, name: &str) -> Vec<String> {
        let tagged = format!("{category}.{name}");
        let counters = self.variant_counters(category);
        let mut references = Vec::new();
        for lane in &self.document.lanes {
            if lane
                .initial
                .as_ref()
                .is_some_and(|initial| names_member(initial, category, Some(name), true))
            {
                references.push(format!("the initial value of the lane '{}'", lane.id));
            }
        }
        for field in &self.document.fields {
            if field
                .initial
                .as_ref()
                .is_some_and(|initial| names_member(initial, category, Some(name), true))
            {
                references.push(format!("the initial value of the field '{}'", field.id));
            }
        }
        for stat in &self.document.stats {
            let counted = counters.iter().any(|(_, node)| *node == stat.from.node);
            if counted && stat.from.port.as_deref() == Some(name) {
                references.push(format!("the statistic '{}'", stat.label));
            }
        }
        for (scope, node) in self.scoped_nodes() {
            let of_category = node.config.get("category").and_then(ConfigValue::as_str) == Some(category);
            let in_inputs = node.inputs.iter().any(|(port, value)| match value {
                InputValue::Literal(literal) => names_member(literal, category, Some(name), true),
                InputValue::Link(link) => {
                    link.port.as_deref() == Some(name) && counters.contains(&(scope.clone(), link.node))
                }
                InputValue::Param(_) => false,
            } || (of_category && port == &format!("case.{name}")));
            let in_config = node.config.iter().any(|(key, value)| {
                holds_tagged(value, "cat", &tagged)
                    || (of_category && matches!(key.as_str(), "dead" | "alive") && value.as_str() == Some(name))
                    || (of_category
                        && key == "rules"
                        && (holds_tagged(value, "from", name) || holds_tagged(value, "to", name)))
            });
            if in_inputs || in_config {
                references.push(node_place(&scope, node.id));
            }
        }
        references
    }

    /// Renames the variant `from` of `category` to `to`, and every reference to it.
    fn rename_variant(&mut self, category: &str, from: &str, to: &str) -> Result<(), EditError> {
        let position = self.variant_position(category, from)?;
        if !is_port_segment(to) {
            return Err(EditError::Invalid(format!("'{to}' is no variant name")));
        }
        if self.variant_position(category, to).is_ok() {
            return Err(EditError::Invalid(format!(
                "the category '{category}' already holds the variant '{to}'"
            )));
        }
        let counters = self.variant_counters(category);
        self.category_mut(category)?.variants[position].name = to.to_owned();
        let rename_literal = |literal: &mut Literal| {
            if let Literal::Category(name) = literal
                && name.owner == category
                && name.member == from
            {
                *name = MemberName {
                    owner: category.to_owned(),
                    member: to.to_owned(),
                };
            }
        };
        for lane in &mut self.document.lanes {
            if let Some(initial) = &mut lane.initial {
                rename_literal(initial);
            }
        }
        for field in &mut self.document.fields {
            if let Some(initial) = &mut field.initial {
                rename_literal(initial);
            }
        }
        for stat in &mut self.document.stats {
            let counted = counters.iter().any(|(_, node)| *node == stat.from.node);
            if counted && stat.from.port.as_deref() == Some(from) {
                stat.from.port = Some(to.to_owned());
            }
        }
        let (old_tag, new_tag) = (format!("{category}.{from}"), format!("{category}.{to}"));
        let passes = self
            .document
            .passes
            .iter_mut()
            .map(|pass| (Scope::Pass(pass.id.clone()), &mut pass.body));
        let functions = self
            .document
            .functions
            .iter_mut()
            .map(|function| (Scope::Function(function.id.clone()), &mut function.body));
        for (scope, body) in passes.chain(functions) {
            for node in &mut body.nodes {
                let of_category = node.config.get("category").and_then(ConfigValue::as_str) == Some(category);
                for value in node.inputs.values_mut() {
                    match value {
                        InputValue::Literal(literal) => rename_literal(literal),
                        InputValue::Link(link) => {
                            if link.port.as_deref() == Some(from) && counters.contains(&(scope.clone(), link.node)) {
                                link.port = Some(to.to_owned());
                            }
                        }
                        InputValue::Param(_) => {}
                    }
                }
                if of_category && let Some(case) = node.inputs.remove(&format!("case.{from}")) {
                    node.inputs.insert(format!("case.{to}"), case);
                }
                for (key, value) in &mut node.config {
                    retag(value, "cat", &old_tag, &new_tag);
                    if !of_category {
                        continue;
                    }
                    match key.as_str() {
                        "dead" | "alive" if value.as_str() == Some(from) => {
                            *value = ConfigValue::String(to.to_owned());
                        }
                        "rules" => {
                            retag(value, "from", from, to);
                            retag(value, "to", from, to);
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }

    /// Returns the position of the parameter `id`.
    fn param_position(&self, id: &str) -> Result<usize, EditError> {
        self.document
            .params
            .iter()
            .position(|param| param.id() == id)
            .ok_or_else(|| EditError::Missing(format!("the file declares no parameter '{id}'")))
    }

    /// Returns each place that binds or names the parameter `id`.
    fn param_references(&self, id: &str) -> Vec<String> {
        let mut references: Vec<String> = self
            .document
            .passes
            .iter()
            .filter(|pass| matches!(&pass.enabled, PassEnabled::Param(param) if param == id))
            .map(|pass| format!("the `enabled` of the pass '{}'", pass.id))
            .collect();
        for (scope, node) in self.scoped_nodes() {
            let bound = node.inputs.values().any(|value| match value {
                InputValue::Param(param) => param == id,
                InputValue::Literal(literal) => names_member(literal, id, None, false),
                InputValue::Link(_) => false,
            });
            let named = node.config.values().any(|value| holds_option(value, id));
            if bound || named {
                references.push(node_place(&scope, node.id));
            }
        }
        references
    }

    /// Renames the parameter `from` to `to`, and every binding of it.
    fn rename_param(&mut self, from: &str, to: &str) -> Result<(), EditError> {
        let position = self.param_position(from)?;
        Self::check_new_id("parameter", to, self.value_ids().filter(|known| *known != from))?;
        self.document.params[position] = renamed_param(&self.document.params[position], to);
        for pass in &mut self.document.passes {
            if let PassEnabled::Param(param) = &mut pass.enabled
                && param == from
            {
                *param = to.to_owned();
            }
        }
        self.for_each_node(|node| {
            for value in node.inputs.values_mut() {
                match value {
                    InputValue::Param(param) if param == from => *param = to.to_owned(),
                    InputValue::Literal(Literal::ChoiceOption(name)) if name.owner == from => {
                        name.owner = to.to_owned();
                    }
                    _ => {}
                }
            }
            for value in node.config.values_mut() {
                rename_option(value, from, to);
            }
        });
        Ok(())
    }

    /// Returns an error unless `label` is a label no statistic holds.
    fn check_new_label(&self, label: &str) -> Result<(), EditError> {
        if label.trim().is_empty() {
            return Err(EditError::Invalid("a statistic needs a label".to_owned()));
        }
        if self.document.stats.iter().any(|stat| stat.label == label) {
            return Err(EditError::Invalid(format!(
                "the file already declares the statistic '{label}'"
            )));
        }
        Ok(())
    }

    /// Returns the position of the statistic `label`.
    fn stat_position(&self, label: &str) -> Result<usize, EditError> {
        self.document
            .stats
            .iter()
            .position(|stat| stat.label == label)
            .ok_or_else(|| EditError::Missing(format!("the file declares no statistic '{label}'")))
    }

    /// Returns the position of the action `id`.
    fn action_position(&self, id: &str) -> Result<usize, EditError> {
        self.document
            .actions
            .iter()
            .position(|action| action.id == id)
            .ok_or_else(|| EditError::Missing(format!("the file declares no action '{id}'")))
    }

    /// Returns the position of the pass `id`.
    fn pass_position(&self, id: &str) -> Result<usize, EditError> {
        self.document
            .passes
            .iter()
            .position(|pass| pass.id == id)
            .ok_or_else(|| EditError::Missing(format!("the file declares no pass '{id}'")))
    }

    /// Renames the pass `from` to `to`, every rerun of it and its canvas.
    fn rename_pass(&mut self, from: &str, to: &str) -> Result<(), EditError> {
        let position = self.pass_position(from)?;
        Self::check_new_id(
            "pass",
            to,
            self.document
                .passes
                .iter()
                .map(|pass| pass.id.as_str())
                .filter(|known| *known != from),
        )?;
        self.document.passes[position].id = to.to_owned();
        for action in &mut self.document.actions {
            for rerun in &mut action.runs {
                if rerun == from {
                    *rerun = to.to_owned();
                }
            }
        }
        self.editor
            .rename_scope(&Scope::pass(from).canvas(), &Scope::pass(to).canvas());
        Ok(())
    }

    /// Removes the pass `id` and its nodes.
    fn remove_pass(&mut self, id: &str) -> Result<(), EditError> {
        let position = self.pass_position(id)?;
        let nodes: Vec<NodeId> = self.document.passes[position]
            .body
            .nodes
            .iter()
            .map(|node| node.id)
            .collect();
        let mut references: Vec<String> = self
            .document
            .actions
            .iter()
            .filter(|action| action.runs.iter().any(|rerun| rerun == id))
            .map(|action| format!("the `runs` of the action '{}'", action.id))
            .collect();
        if self.document.passes[position].stage == PassStage::Stats {
            references.extend(
                self.document
                    .stats
                    .iter()
                    .filter(|stat| nodes.contains(&stat.from.node))
                    .map(|stat| format!("the statistic '{}'", stat.label)),
            );
        }
        Self::refuse_referenced(format!("the pass '{id}'"), references)?;
        self.document.passes.remove(position);
        for node in nodes {
            self.editor.remove_node(node);
        }
        self.editor.remove_scope(&Scope::pass(id).canvas());
        Ok(())
    }

    /// Moves the pass `id` to position `index` among the passes of its stage.
    fn move_pass(&mut self, id: &str, index: usize) -> Result<(), EditError> {
        let position = self.pass_position(id)?;
        let pass = self.document.passes.remove(position);
        let stage = pass.stage;
        let first = self
            .document
            .passes
            .iter()
            .position(|known| known.stage >= stage)
            .unwrap_or(self.document.passes.len());
        let count = self.document.passes[first..]
            .iter()
            .take_while(|known| known.stage == stage)
            .count();
        self.document.passes.insert(first + index.min(count), pass);
        Ok(())
    }
}

/// Returns whether `value` holds an option literal `{ option = "<param>.<name>" }` of the parameter `param`.
fn holds_option(value: &ConfigValue, param: &str) -> bool {
    match value {
        ConfigValue::Array(items) => items.iter().any(|item| holds_option(item, param)),
        ConfigValue::Table(table) => table.iter().any(|(key, item)| {
            (key == "option"
                && item
                    .as_str()
                    .and_then(|text| text.split_once('.'))
                    .is_some_and(|(owner, _)| owner == param))
                || holds_option(item, param)
        }),
        _ => false,
    }
}

/// Rewrites every option literal of the parameter `from` inside `value` to name the parameter `to`.
fn rename_option(value: &mut ConfigValue, from: &str, to: &str) {
    match value {
        ConfigValue::Array(items) => {
            for item in items {
                rename_option(item, from, to);
            }
        }
        ConfigValue::Table(table) => {
            for (key, item) in table.iter_mut() {
                if key == "option"
                    && let ConfigValue::String(text) = item
                    && let Some((owner, member)) = text.split_once('.')
                    && owner == from
                {
                    *text = format!("{to}.{member}");
                } else {
                    rename_option(item, from, to);
                }
            }
        }
        _ => {}
    }
}
