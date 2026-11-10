//! The `[editor]` region of a graph file: presentation only, outside the code identity.
//!
//! The region holds node placement, canvas views, frames and notes. It is read leniently. Any TOML value is kept
//! through a round trip, unknown keys included, and a key that does not fit reads as absent through the accessors.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::ids::NodeId;
use crate::model_text::toml_string;

/// Keys of the `[editor]` table written before every other key, in order.
const LEADING_KEYS: [&str; 5] = ["version", "next_id", "node", "view", "function"];
/// Keys of the `[editor]` table written as arrays of tables after every other key, in order.
const TRAILING_TABLES: [&str; 2] = ["frame", "note"];
/// Deepest nesting of arrays and inline tables in one value the region writes.
///
/// A value nested deeper is left out of the text. TOML readers refuse nesting past 80, and the region writes a table
/// that a file spelt with dotted keys as one inline value.
const MAX_INLINE_DEPTH: usize = 64;
/// Keys of a table inside the region, in the order they are written. A key not listed follows, in lexical order.
const INLINE_KEY_ORDER: [&str; 12] = [
    "scope",
    "title",
    "color",
    "members",
    "pos",
    "output_pos",
    "collapsed",
    "label",
    "text",
    "pan",
    "zoom",
    "name",
];

/// The `[editor]` region of a graph file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EditorRegion {
    table: toml::Table,
}

/// Placement of one node on its canvas.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct NodeLayout {
    /// Position on the canvas in points.
    pub pos: [f64; 2],
    /// Whether the node is drawn collapsed.
    pub collapsed: bool,
    /// Display label, `None` to show the kind's own title.
    pub label: Option<String>,
    /// Position on the canvas in points of a region's output half, `None` when the region has none placed.
    pub output_pos: Option<[f64; 2]>,
}

/// A titled frame drawn behind some nodes of one canvas.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct FrameLayout {
    /// Canvas the frame is on, such as `pass.step`.
    pub scope: String,
    /// Title.
    pub title: String,
    /// MCS hue name, such as `blue`.
    pub color: String,
    /// Nodes inside the frame. A member that is no node id is left out.
    pub members: Vec<NodeId>,
    /// Origins the frame's nodes came from in the sheet a converted copy was made from, read from `line` (one id) or
    /// `lines` (several), empty for a frame drawn by hand.
    ///
    /// Each is the id of a line, such as `l2`, or the path of a part of the sheet that no line owns, such as
    /// `cells.status.starts_as`, `measure.alive` or `button.clear`.
    pub lines: Vec<String>,
}

impl FrameLayout {
    /// Creates the frame titled `title` in `color`, an MCS hue name, around `members` of the canvas `scope`, with no
    /// origins.
    pub fn new(scope: &str, title: &str, color: &str, members: Vec<NodeId>) -> Self {
        Self {
            scope: scope.to_owned(),
            title: title.to_owned(),
            color: color.to_owned(),
            members,
            lines: Vec::new(),
        }
    }

    /// Returns the frame with the origins `lines`.
    pub fn with_lines(mut self, lines: Vec<String>) -> Self {
        self.lines = lines;
        self
    }
}

/// A note of the region: text beside a canvas, or about the model as a whole.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct NoteLayout {
    /// Canvas the note is on, such as `pass.step`, empty for a note about the model as a whole.
    pub scope: String,
    /// Position on the canvas in points, `None` for a note placed on no canvas.
    pub pos: Option<[f64; 2]>,
    /// Title, empty for a note with none.
    pub title: String,
    /// Text.
    pub text: String,
}

impl NoteLayout {
    /// Creates the note `text` titled `title`, about the model as a whole and placed on no canvas.
    pub fn about_model(title: &str, text: &str) -> Self {
        Self {
            scope: String::new(),
            pos: None,
            title: title.to_owned(),
            text: text.to_owned(),
        }
    }
}

/// Source of a graph that a conversion copied from a sheet, the `origin` key of the region.
///
/// The record stays outside the code identity, as the whole region does.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ModelOrigin {
    /// Kind of model file the graph came from, `sheet`.
    pub kind: String,
    /// Model id of the source at conversion.
    pub model: String,
    /// Name of the source at conversion.
    pub name: String,
    /// File name of the source, relative to the graph's file.
    pub file: String,
    /// FNV-1a hash of the source's canonical text at conversion, as 16 lowercase hexadecimal digits.
    pub hash: String,
    /// Hash of the source's code identity at conversion, as 16 lowercase hexadecimal digits.
    pub code: String,
    /// Hash of the graph's own code identity at conversion, as 16 lowercase hexadecimal digits.
    pub copy_code: String,
    /// Date of the conversion, such as `2026-11-07`.
    pub converted: String,
}

impl ModelOrigin {
    /// Keys of the record, in the order of its fields.
    const KEYS: [&'static str; 8] = [
        "kind",
        "model",
        "name",
        "file",
        "hash",
        "code",
        "copy_code",
        "converted",
    ];

    /// Returns the values of the record, in the order of [`Self::KEYS`].
    fn values(&self) -> [&str; 8] {
        [
            &self.kind,
            &self.model,
            &self.name,
            &self.file,
            &self.hash,
            &self.code,
            &self.copy_code,
            &self.converted,
        ]
    }
}

/// Source of a function that Import function copied into the file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct FunctionOrigin {
    /// Name of the model file the function came from.
    pub name: String,
    /// FNV-1a hash of the function's canonical text in that file, as 16 lowercase hexadecimal digits.
    pub hash: String,
}

impl EditorRegion {
    /// Returns the region that `table` holds.
    pub fn from_table(table: toml::Table) -> Self {
        Self { table }
    }

    /// Whole table of the region, unknown keys included.
    pub fn table(&self) -> &toml::Table {
        &self.table
    }

    /// Mutable access to the whole table of the region.
    pub fn table_mut(&mut self) -> &mut toml::Table {
        &mut self.table
    }

    /// Next node number the file records, `None` when the key is absent or does not fit a `u32`.
    pub fn next_id(&self) -> Option<u32> {
        let number = self.table.get("next_id")?.as_integer()?;
        u32::try_from(number).ok()
    }

    /// Placement of node `id`, `None` when the region holds none. A key that does not fit takes its default.
    pub fn node_layout(&self, id: NodeId) -> Option<NodeLayout> {
        let entry = self.node_entry(id)?;
        let point = |key: &str| entry.get(key).and_then(point);
        Some(NodeLayout {
            pos: point("pos").unwrap_or_default(),
            collapsed: entry
                .get("collapsed")
                .and_then(toml::Value::as_bool)
                .unwrap_or_default(),
            label: entry.get("label").and_then(toml::Value::as_str).map(str::to_owned),
            output_pos: point("output_pos"),
        })
    }

    /// Position of node `id` on its canvas in points, `None` for a node the region does not place.
    ///
    /// Note that [`Self::node_layout`] returns a layout at `[0, 0]` for a node that has a label or another key and no
    /// position, as a converted copy's labelled nodes do. A node with no position here is unplaced.
    pub fn node_pos(&self, id: NodeId) -> Option<[f64; 2]> {
        self.node_entry(id)?.get("pos").and_then(point)
    }

    /// Returns the `node.<id>` table of node `id`, `None` when the region holds none.
    fn node_entry(&self, id: NodeId) -> Option<&toml::Table> {
        self.table.get("node")?.as_table()?.get(&id.to_string())?.as_table()
    }

    /// Returns the `node.<id>` table of node `id` for writing, added empty when the region holds none.
    fn node_entry_mut(&mut self, id: NodeId) -> &mut toml::Table {
        table_entry(table_entry(&mut self.table, "node"), &id.to_string())
    }

    /// Origin of the function `id`, `None` when the region records none or a key does not fit.
    pub fn function_origin(&self, id: &str) -> Option<FunctionOrigin> {
        let origin = self
            .table
            .get("function")?
            .as_table()?
            .get(id)?
            .as_table()?
            .get("origin")?
            .as_table()?;
        Some(FunctionOrigin {
            name: origin.get("name")?.as_str()?.to_owned(),
            hash: origin.get("hash")?.as_str()?.to_owned(),
        })
    }

    /// Records `origin` as the origin of the function `id`, keeping its other keys.
    pub fn set_function_origin(&mut self, id: &str, origin: &FunctionOrigin) {
        let functions = table_entry(&mut self.table, "function");
        let entry = table_entry(functions, id);
        let mut table = toml::Table::new();
        table.insert("name".to_owned(), toml::Value::String(origin.name.clone()));
        table.insert("hash".to_owned(), toml::Value::String(origin.hash.clone()));
        entry.insert("origin".to_owned(), toml::Value::Table(table));
    }

    /// Source the graph was converted from, `None` when the region records none or a key does not fit.
    pub fn model_origin(&self) -> Option<ModelOrigin> {
        let origin = self.table.get("origin")?.as_table()?;
        let text = |key: &str| origin.get(key).and_then(toml::Value::as_str).map(str::to_owned);
        Some(ModelOrigin {
            kind: text("kind")?,
            model: text("model")?,
            name: text("name")?,
            file: text("file")?,
            hash: text("hash")?,
            code: text("code")?,
            copy_code: text("copy_code")?,
            converted: text("converted")?,
        })
    }

    /// Records `origin` as the source the graph was converted from, in place of any record before.
    pub fn set_model_origin(&mut self, origin: &ModelOrigin) {
        let table: toml::Table = ModelOrigin::KEYS
            .iter()
            .zip(origin.values())
            .map(|(key, value)| ((*key).to_owned(), toml::Value::String(value.to_owned())))
            .collect();
        self.table.insert("origin".to_owned(), toml::Value::Table(table));
    }

    /// Moves the entry of the function `from` to `to`, and renames its canvas scope.
    pub fn rename_function(&mut self, from: &str, to: &str) {
        if let Some(functions) = self.table.get_mut("function").and_then(toml::Value::as_table_mut)
            && let Some(entry) = functions.remove(from)
        {
            functions.insert(to.to_owned(), entry);
        }
        self.rename_scope(&format!("function.{from}"), &format!("function.{to}"));
    }

    /// Removes the entry of the function `id`, and the view, frames and notes of its canvas.
    pub fn remove_function(&mut self, id: &str) {
        if let Some(functions) = self.table.get_mut("function").and_then(toml::Value::as_table_mut) {
            functions.remove(id);
        }
        self.remove_scope(&format!("function.{id}"));
    }

    /// Returns every `[[editor.frame]]` in file order. A key that does not fit takes its default.
    pub fn frames(&self) -> Vec<FrameLayout> {
        let Some(frames) = self.table.get("frame").and_then(toml::Value::as_array) else {
            return Vec::new();
        };
        frames
            .iter()
            .filter_map(toml::Value::as_table)
            .map(|frame| FrameLayout {
                scope: text(frame, "scope"),
                title: text(frame, "title"),
                color: text(frame, "color"),
                members: frame
                    .get("members")
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|member| member.as_str()?.parse().ok())
                    .collect(),
                lines: frame_lines(frame),
            })
            .collect()
    }

    /// Returns every `[[editor.note]]` in file order. A key that does not fit takes its default.
    pub fn notes(&self) -> Vec<NoteLayout> {
        let Some(notes) = self.table.get("note").and_then(toml::Value::as_array) else {
            return Vec::new();
        };
        notes
            .iter()
            .filter_map(toml::Value::as_table)
            .map(|note| NoteLayout {
                scope: text(note, "scope"),
                pos: note.get("pos").and_then(point),
                title: text(note, "title"),
                text: text(note, "text"),
            })
            .collect()
    }

    /// Adds `note` after the notes of the region, writing `scope`, `pos` and `title` only when they are set.
    pub fn add_note(&mut self, note: &NoteLayout) {
        let mut table = toml::Table::new();
        if !note.scope.is_empty() {
            table.insert("scope".to_owned(), toml::Value::String(note.scope.clone()));
        }
        if let Some(pos) = note.pos {
            table.insert("pos".to_owned(), point_value(pos));
        }
        if !note.title.is_empty() {
            table.insert("title".to_owned(), toml::Value::String(note.title.clone()));
        }
        table.insert("text".to_owned(), toml::Value::String(note.text.clone()));
        push_table(&mut self.table, "note", table);
    }

    /// Sets the next node number to `next`.
    pub fn set_next_id(&mut self, next: u32) {
        self.table
            .insert("next_id".to_owned(), toml::Value::Integer(i64::from(next)));
    }

    /// Sets the position of node `id` to `pos`, keeping its other placement keys.
    pub fn set_node_pos(&mut self, id: NodeId, pos: [f64; 2]) {
        self.node_entry_mut(id).insert("pos".to_owned(), point_value(pos));
    }

    /// Sets the position of the output half of the region `id` to `pos`, keeping its other placement keys.
    pub fn set_output_pos(&mut self, id: NodeId, pos: [f64; 2]) {
        self.node_entry_mut(id)
            .insert("output_pos".to_owned(), point_value(pos));
    }

    /// Sets the placement of node `id` to `layout`, writing `collapsed`, `label` and `output_pos` only when they are
    /// set.
    pub fn place_node(&mut self, id: NodeId, layout: &NodeLayout) {
        self.set_node_pos(id, layout.pos);
        if let Some(pos) = layout.output_pos {
            self.set_output_pos(id, pos);
        }
        let entry = self.node_entry_mut(id);
        if layout.collapsed {
            entry.insert("collapsed".to_owned(), toml::Value::Boolean(true));
        }
        if let Some(label) = &layout.label {
            entry.insert("label".to_owned(), toml::Value::String(label.clone()));
        }
    }

    /// Sets the label of node `id` to `label`, keeping its other placement keys.
    pub fn set_node_label(&mut self, id: NodeId, label: &str) {
        self.node_entry_mut(id)
            .insert("label".to_owned(), toml::Value::String(label.to_owned()));
    }

    /// Removes the placement of node `id`, and removes the node from every frame.
    pub fn remove_node(&mut self, id: NodeId) {
        let name = id.to_string();
        if let Some(nodes) = self.table.get_mut("node").and_then(toml::Value::as_table_mut) {
            nodes.remove(&name);
        }
        if let Some(frames) = self.table.get_mut("frame").and_then(toml::Value::as_array_mut) {
            for frame in frames.iter_mut().filter_map(toml::Value::as_table_mut) {
                if let Some(members) = frame.get_mut("members").and_then(toml::Value::as_array_mut) {
                    members.retain(|member| member.as_str() != Some(name.as_str()));
                }
            }
        }
    }

    /// Adds a frame titled `title` in `color`, an MCS hue name, around `members` of the canvas `scope`.
    pub fn add_frame(&mut self, scope: &str, title: &str, color: &str, members: &[NodeId]) {
        self.push_frame(&FrameLayout::new(scope, title, color, members.to_vec()));
    }

    /// Adds `frame` after the frames of the region, writing its origins as `line` when it has one and as `lines` when
    /// it has several.
    pub fn push_frame(&mut self, frame: &FrameLayout) {
        let mut table = toml::Table::new();
        table.insert("scope".to_owned(), toml::Value::String(frame.scope.clone()));
        table.insert("title".to_owned(), toml::Value::String(frame.title.clone()));
        table.insert("color".to_owned(), toml::Value::String(frame.color.clone()));
        table.insert("members".to_owned(), members_value(&frame.members));
        match frame.lines.as_slice() {
            [] => {}
            [line] => {
                table.insert("line".to_owned(), toml::Value::String(line.clone()));
            }
            lines => {
                table.insert(
                    "lines".to_owned(),
                    toml::Value::Array(lines.iter().cloned().map(toml::Value::String).collect()),
                );
            }
        }
        push_table(&mut self.table, "frame", table);
    }

    /// Moves the members `moved` of every frame of the canvas `scope` into a frame of the canvas `dest`, and puts
    /// `replacement` in their place.
    ///
    /// Each frame of `scope` that holds a node of `moved` keeps its other members and takes `replacement`, and a copy
    /// of it on `dest`, every other key kept, holds the members it lost. Make function calls this with the moved nodes,
    /// the function's canvas and the call that takes their place.
    pub fn move_frame_members(&mut self, scope: &str, moved: &[NodeId], dest: &str, replacement: NodeId) {
        let Some(frames) = self.table.get_mut("frame").and_then(toml::Value::as_array_mut) else {
            return;
        };
        let moved: Vec<String> = moved.iter().map(NodeId::to_string).collect();
        let replacement = toml::Value::String(replacement.to_string());
        let mut copies = Vec::new();
        for frame in frames.iter_mut().filter_map(toml::Value::as_table_mut) {
            if frame.get("scope").and_then(toml::Value::as_str) != Some(scope) {
                continue;
            }
            let Some(members) = frame.get_mut("members").and_then(toml::Value::as_array_mut) else {
                continue;
            };
            let (left, kept): (Vec<toml::Value>, Vec<toml::Value>) =
                std::mem::take(members).into_iter().partition(|member| {
                    member
                        .as_str()
                        .is_some_and(|name| moved.iter().any(|known| known == name))
                });
            *members = kept;
            if left.is_empty() {
                continue;
            }
            if !members.contains(&replacement) {
                members.push(replacement.clone());
            }
            let mut copy = frame.clone();
            copy.insert("scope".to_owned(), toml::Value::String(dest.to_owned()));
            copy.insert("members".to_owned(), toml::Value::Array(left));
            copies.push(toml::Value::Table(copy));
        }
        frames.extend(copies);
    }

    /// Returns the region of a copy of `nodes` of the canvas `scope`: their placements, and each frame that holds one
    /// of them with only those members, every other key kept.
    pub(crate) fn copy_of(&self, scope: &str, nodes: &[NodeId]) -> Self {
        let mut copy = Self::default();
        if let Some(placed) = self.table.get("node").and_then(toml::Value::as_table) {
            let mut kept = toml::Table::new();
            for node in nodes {
                if let Some(entry) = placed.get(&node.to_string()) {
                    kept.insert(node.to_string(), entry.clone());
                }
            }
            copy.table.insert("node".to_owned(), toml::Value::Table(kept));
        }
        for frame in self.frames_holding(scope, nodes) {
            push_table(&mut copy.table, "frame", frame);
        }
        copy
    }

    /// Adds a frame on the canvas `scope` for each frame of `source`, every key kept, around the members that
    /// `renamed` maps, each renamed. A frame with no such member is left out.
    pub(crate) fn push_renamed_frames(&mut self, source: &Self, scope: &str, renamed: &BTreeMap<NodeId, NodeId>) {
        let Some(frames) = source.table.get("frame").and_then(toml::Value::as_array) else {
            return;
        };
        for frame in frames.iter().filter_map(toml::Value::as_table) {
            let members = renamed_members(frame, renamed);
            if members.is_empty() {
                continue;
            }
            let mut pasted = frame.clone();
            pasted.insert("scope".to_owned(), toml::Value::String(scope.to_owned()));
            pasted.insert("members".to_owned(), toml::Value::Array(members));
            push_table(&mut self.table, "frame", pasted);
        }
    }

    /// Writes `version = 1` when the region records no version.
    pub(crate) fn ensure_version(&mut self) {
        if self.table.get("version").is_none() {
            self.table.insert("version".to_owned(), toml::Value::Integer(1));
        }
    }

    /// Returns the frames of the canvas `scope` that hold a node of `nodes`, each holding only those nodes, as tables
    /// with every key kept.
    fn frames_holding(&self, scope: &str, nodes: &[NodeId]) -> Vec<toml::Table> {
        let Some(frames) = self.table.get("frame").and_then(toml::Value::as_array) else {
            return Vec::new();
        };
        let names: Vec<String> = nodes.iter().map(NodeId::to_string).collect();
        frames
            .iter()
            .filter_map(toml::Value::as_table)
            .filter(|frame| frame.get("scope").and_then(toml::Value::as_str) == Some(scope))
            .filter_map(|frame| {
                let members: Vec<toml::Value> = frame
                    .get("members")
                    .and_then(toml::Value::as_array)?
                    .iter()
                    .filter(|member| {
                        member
                            .as_str()
                            .is_some_and(|name| names.iter().any(|known| known == name))
                    })
                    .cloned()
                    .collect();
                if members.is_empty() {
                    return None;
                }
                let mut kept = frame.clone();
                kept.insert("members".to_owned(), toml::Value::Array(members));
                Some(kept)
            })
            .collect()
    }

    /// Renames the canvas scope `from`, such as `pass.seed`, to `to` in views, frames and notes.
    pub fn rename_scope(&mut self, from: &str, to: &str) {
        if let Some(views) = self.table.get_mut("view").and_then(toml::Value::as_table_mut)
            && let Some(view) = views.remove(from)
        {
            views.insert(to.to_owned(), view);
        }
        for name in TRAILING_TABLES {
            if let Some(items) = self.table.get_mut(name).and_then(toml::Value::as_array_mut) {
                for item in items.iter_mut().filter_map(toml::Value::as_table_mut) {
                    if item.get("scope").and_then(toml::Value::as_str) == Some(from) {
                        item.insert("scope".to_owned(), toml::Value::String(to.to_owned()));
                    }
                }
            }
        }
    }

    /// Removes the view, frames and notes of the canvas scope `scope`.
    pub fn remove_scope(&mut self, scope: &str) {
        if let Some(views) = self.table.get_mut("view").and_then(toml::Value::as_table_mut) {
            views.remove(scope);
        }
        for name in TRAILING_TABLES {
            if let Some(items) = self.table.get_mut(name).and_then(toml::Value::as_array_mut) {
                items.retain(|item| item.get("scope").and_then(toml::Value::as_str) != Some(scope));
            }
        }
    }

    /// Returns the region's canonical text, starting with its `[editor]` header.
    ///
    /// Keys follow a fixed order, `node.<id>` entries natural id order, and positions are rounded to whole points.
    pub(crate) fn to_text(&self) -> String {
        let mut text = String::from("[editor]\n");
        for name in ["version", "next_id"] {
            if let Some(value) = self.table.get(name) {
                push_entry(&mut text, name, value);
            }
        }
        self.write_entries(&mut text, "node");
        self.write_entries(&mut text, "view");
        self.write_function_entries(&mut text);
        let mut others: Vec<(&String, &toml::Value)> = self
            .table
            .iter()
            .filter(|(name, value)| !is_written_apart(name, value))
            .collect();
        others.sort_by(|left, right| left.0.cmp(right.0));
        for (name, value) in others {
            push_entry(&mut text, &toml_key(name), value);
        }
        for name in TRAILING_TABLES {
            if let Some(items) = self.table.get(name).and_then(array_of_tables) {
                for item in items {
                    text.push_str(&format!("\n[[editor.{name}]]\n"));
                    for (key, value) in ordered_entries(item) {
                        let value = if key == "pos" { rounded(value) } else { value.clone() };
                        push_entry(&mut text, &toml_key(key), &value);
                    }
                }
            }
        }
        text
    }

    /// Writes the entries of the table `name` as dotted keys, in natural order.
    fn write_entries(&self, text: &mut String, name: &str) {
        let Some(table) = self.table.get(name).and_then(toml::Value::as_table) else {
            return;
        };
        let mut entries: Vec<(&String, &toml::Value)> = table.iter().collect();
        entries.sort_by(|left, right| natural_order(left.0, right.0));
        for (key, value) in entries {
            let key = format!("{name}.{}", toml_key(key));
            if name == "node" {
                push_entry(text, &key, &node_with_rounded_pos(value));
            } else {
                push_entry(text, &key, value);
            }
        }
    }

    /// Writes the `function` table as `function.<id>.<key>` lines.
    fn write_function_entries(&self, text: &mut String) {
        let Some(table) = self.table.get("function").and_then(toml::Value::as_table) else {
            return;
        };
        let mut functions: Vec<(&String, &toml::Value)> = table.iter().collect();
        functions.sort_by(|left, right| left.0.cmp(right.0));
        for (id, value) in functions {
            match value.as_table().filter(|entry| !entry.is_empty()) {
                Some(entry) => {
                    for (key, item) in ordered_entries(entry) {
                        push_entry(text, &format!("function.{}.{}", toml_key(id), toml_key(key)), item);
                    }
                }
                None => push_entry(text, &format!("function.{}", toml_key(id)), value),
            }
        }
    }
}

/// Returns the table under `key` of `table`, made a table first when it is absent or holds another value.
fn table_entry<'a>(table: &'a mut toml::Table, key: &str) -> &'a mut toml::Table {
    let value = table
        .entry(key)
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if !value.is_table() {
        *value = toml::Value::Table(toml::Table::new());
    }
    match value {
        toml::Value::Table(inner) => inner,
        _ => unreachable!("the value was made a table above"),
    }
}

/// Returns the origins of `frame`: its `line` string, or each string of its `lines` array.
fn frame_lines(frame: &toml::Table) -> Vec<String> {
    if let Some(line) = frame.get("line").and_then(toml::Value::as_str) {
        return vec![line.to_owned()];
    }
    frame
        .get("lines")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|line| line.as_str().map(str::to_owned))
        .collect()
}

/// Returns the member ids of the frame table `frame` that `renamed` maps, each renamed.
fn renamed_members(frame: &toml::Table, renamed: &BTreeMap<NodeId, NodeId>) -> Vec<toml::Value> {
    frame
        .get("members")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|member| member.as_str()?.parse::<NodeId>().ok())
        .filter_map(|member| renamed.get(&member))
        .map(|member| toml::Value::String(member.to_string()))
        .collect()
}

/// Returns the string at `key` of `table`, empty when the key is absent or holds no string.
fn text(table: &toml::Table, key: &str) -> String {
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// Returns `pos` as the `[x, y]` array of floats that [`point`] reads.
fn point_value(pos: [f64; 2]) -> toml::Value {
    toml::Value::Array(vec![toml::Value::Float(pos[0]), toml::Value::Float(pos[1])])
}

/// Returns `value` as a point when it is an array of two numbers.
fn point(value: &toml::Value) -> Option<[f64; 2]> {
    let number = |value: &toml::Value| {
        value
            .as_float()
            .or_else(|| value.as_integer().map(|whole| whole as f64))
    };
    match value.as_array()?.as_slice() {
        [x, y] => Some([number(x)?, number(y)?]),
        _ => None,
    }
}

/// Returns `members` as an array of node id strings.
fn members_value(members: &[NodeId]) -> toml::Value {
    toml::Value::Array(
        members
            .iter()
            .map(|member| toml::Value::String(member.to_string()))
            .collect(),
    )
}

/// Appends `item` to the array of tables under `key` of `table`, made an empty array first when it is absent or holds
/// another value.
fn push_table(table: &mut toml::Table, key: &str, item: toml::Table) {
    let items = table.entry(key).or_insert_with(|| toml::Value::Array(Vec::new()));
    if !items.is_array() {
        *items = toml::Value::Array(Vec::new());
    }
    if let Some(items) = items.as_array_mut() {
        items.push(toml::Value::Table(item));
    }
}

/// Returns whether the key `name` is written by a dedicated step of [`EditorRegion::to_text`].
fn is_written_apart(name: &str, value: &toml::Value) -> bool {
    if name == "version" || name == "next_id" {
        return true;
    }
    if LEADING_KEYS.contains(&name) && value.is_table() {
        return true;
    }
    TRAILING_TABLES.contains(&name) && array_of_tables(value).is_some()
}

/// Returns the elements of `value` when it is a non-empty array that holds only tables.
fn array_of_tables(value: &toml::Value) -> Option<Vec<&toml::Table>> {
    let items = value.as_array().filter(|items| !items.is_empty())?;
    items.iter().map(toml::Value::as_table).collect()
}

/// Returns `entry` with its `pos` and `output_pos` keys rounded, when it is a table that holds them.
fn node_with_rounded_pos(entry: &toml::Value) -> toml::Value {
    let mut entry = entry.clone();
    if let Some(table) = entry.as_table_mut() {
        for key in ["pos", "output_pos"] {
            if let Some(pos) = table.get_mut(key) {
                *pos = rounded(pos);
            }
        }
    }
    entry
}

/// Returns the array `value` with its finite numbers rounded to whole points, written as floats.
fn rounded(value: &toml::Value) -> toml::Value {
    let Some(items) = value.as_array() else {
        return value.clone();
    };
    let round = |number: f64| toml::Value::Float(number.round() + 0.0);
    toml::Value::Array(
        items
            .iter()
            .map(|item| match item {
                toml::Value::Float(number) if number.is_finite() => round(*number),
                toml::Value::Integer(number) => round(*number as f64),
                other => other.clone(),
            })
            .collect(),
    )
}

/// Returns the entries of `table` in the order the region writes them.
fn ordered_entries(table: &toml::Table) -> Vec<(&String, &toml::Value)> {
    let rank = |name: &str| {
        INLINE_KEY_ORDER
            .iter()
            .position(|listed| *listed == name)
            .unwrap_or(INLINE_KEY_ORDER.len())
    };
    let mut entries: Vec<(&String, &toml::Value)> = table.iter().collect();
    entries.sort_by(|left, right| rank(left.0).cmp(&rank(right.0)).then_with(|| left.0.cmp(right.0)));
    entries
}

/// Orders two keys: node ids by number first, then every other key lexically.
fn natural_order(left: &str, right: &str) -> Ordering {
    match (left.parse::<NodeId>(), right.parse::<NodeId>()) {
        (Ok(left), Ok(right)) => left.cmp(&right),
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => left.cmp(right),
    }
}

/// Returns `key` bare when TOML allows it, and quoted otherwise.
fn toml_key(key: &str) -> String {
    let bare = !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if bare { key.to_owned() } else { toml_string(key) }
}

/// Appends the line `key = value` to `text`, or nothing for a value nested deeper than [`MAX_INLINE_DEPTH`].
fn push_entry(text: &mut String, key: &str, value: &toml::Value) {
    if nests_within(value, MAX_INLINE_DEPTH) {
        text.push_str(&format!("{key} = {}\n", inline_value(value)));
    }
}

/// Returns whether `value` nests arrays and inline tables at most `depth` deep.
fn nests_within(value: &toml::Value, depth: usize) -> bool {
    match value {
        toml::Value::Array(items) => depth > 0 && items.iter().all(|item| nests_within(item, depth - 1)),
        toml::Value::Table(table) => depth > 0 && table.values().all(|item| nests_within(item, depth - 1)),
        _ => true,
    }
}

/// Returns `value` on one line.
fn inline_value(value: &toml::Value) -> String {
    match value {
        toml::Value::String(text) => toml_string(text),
        toml::Value::Integer(number) => number.to_string(),
        toml::Value::Float(number) => float_text(*number),
        toml::Value::Boolean(flag) => flag.to_string(),
        toml::Value::Datetime(datetime) => datetime.to_string(),
        toml::Value::Array(items) => {
            let written: Vec<String> = items.iter().map(inline_value).collect();
            format!("[{}]", written.join(", "))
        }
        toml::Value::Table(table) => {
            if table.is_empty() {
                return "{}".to_owned();
            }
            let written: Vec<String> = ordered_entries(table)
                .into_iter()
                .map(|(key, item)| format!("{} = {}", toml_key(key), inline_value(item)))
                .collect();
            format!("{{ {} }}", written.join(", "))
        }
    }
}

/// Returns `number` in TOML's spelling. The region keeps a float that is not finite as a file wrote it.
fn float_text(number: f64) -> String {
    if number.is_nan() {
        "nan".to_owned()
    } else if number.is_infinite() {
        if number > 0.0 { "inf" } else { "-inf" }.to_owned()
    } else {
        format!("{number:?}")
    }
}
