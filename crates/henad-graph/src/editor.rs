//! The `[editor]` region of a graph file: presentation only, outside the code identity.
//!
//! The region holds node placement, canvas views, frames and notes. It is read leniently. Any TOML value is kept
//! through a round trip, unknown keys included, and a key that does not fit reads as absent through the accessors.

use std::cmp::Ordering;

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
        let entry = self.table.get("node")?.as_table()?.get(&id.to_string())?.as_table()?;
        let number = |value: &toml::Value| {
            value
                .as_float()
                .or_else(|| value.as_integer().map(|whole| whole as f64))
        };
        let point = |key: &str| -> Option<[f64; 2]> {
            match entry.get(key)?.as_array()?.as_slice() {
                [x, y] => Some([number(x)?, number(y)?]),
                _ => None,
            }
        };
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
        let text = |frame: &toml::Table, key: &str| {
            frame
                .get(key)
                .and_then(toml::Value::as_str)
                .unwrap_or_default()
                .to_owned()
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
            })
            .collect()
    }

    /// Sets the next node number to `next`.
    pub fn set_next_id(&mut self, next: u32) {
        self.table
            .insert("next_id".to_owned(), toml::Value::Integer(i64::from(next)));
    }

    /// Sets the position of node `id` to `pos`, keeping its other placement keys.
    pub fn set_node_pos(&mut self, id: NodeId, pos: [f64; 2]) {
        let nodes = table_entry(&mut self.table, "node");
        let entry = table_entry(nodes, &id.to_string());
        entry.insert(
            "pos".to_owned(),
            toml::Value::Array(vec![toml::Value::Float(pos[0]), toml::Value::Float(pos[1])]),
        );
    }

    /// Sets the position of the output half of the region `id` to `pos`, keeping its other placement keys.
    pub fn set_output_pos(&mut self, id: NodeId, pos: [f64; 2]) {
        let nodes = table_entry(&mut self.table, "node");
        let entry = table_entry(nodes, &id.to_string());
        entry.insert(
            "output_pos".to_owned(),
            toml::Value::Array(vec![toml::Value::Float(pos[0]), toml::Value::Float(pos[1])]),
        );
    }

    /// Sets the placement of node `id` to `layout`, writing `collapsed`, `label` and `output_pos` only when they are
    /// set.
    pub fn place_node(&mut self, id: NodeId, layout: &NodeLayout) {
        self.set_node_pos(id, layout.pos);
        if let Some(pos) = layout.output_pos {
            self.set_output_pos(id, pos);
        }
        let entry = table_entry(table_entry(&mut self.table, "node"), &id.to_string());
        if layout.collapsed {
            entry.insert("collapsed".to_owned(), toml::Value::Boolean(true));
        }
        if let Some(label) = &layout.label {
            entry.insert("label".to_owned(), toml::Value::String(label.clone()));
        }
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
        let mut frame = toml::Table::new();
        frame.insert("scope".to_owned(), toml::Value::String(scope.to_owned()));
        frame.insert("title".to_owned(), toml::Value::String(title.to_owned()));
        frame.insert("color".to_owned(), toml::Value::String(color.to_owned()));
        frame.insert(
            "members".to_owned(),
            toml::Value::Array(
                members
                    .iter()
                    .map(|member| toml::Value::String(member.to_string()))
                    .collect(),
            ),
        );
        let frames = self
            .table
            .entry("frame")
            .or_insert_with(|| toml::Value::Array(Vec::new()));
        if !frames.is_array() {
            *frames = toml::Value::Array(Vec::new());
        }
        if let Some(frames) = frames.as_array_mut() {
            frames.push(toml::Value::Table(frame));
        }
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
