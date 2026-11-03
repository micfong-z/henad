//! The `[editor]` region of a graph file: presentation only, outside the code identity.
//!
//! The region holds node placement, canvas views, frames and notes. It is read leniently. Any TOML value is kept through
//! a round trip, unknown keys included, and a key that does not fit reads as absent through the accessors.

use std::cmp::Ordering;

use crate::ids::NodeId;
use crate::model_text::toml_string;

/// Keys of the `[editor]` table written before every other key, in order.
const LEADING_KEYS: [&str; 5] = ["version", "next_id", "node", "view", "function"];
/// Keys of the `[editor]` table written as arrays of tables after every other key, in order.
const TRAILING_TABLES: [&str; 2] = ["frame", "note"];
/// Keys of a table inside the region, in the order they are written. A key not listed follows, in lexical order.
const INLINE_KEY_ORDER: [&str; 11] = [
    "scope",
    "title",
    "color",
    "members",
    "pos",
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
        let pos = entry
            .get("pos")
            .and_then(toml::Value::as_array)
            .and_then(|pos| match pos.as_slice() {
                [x, y] => Some([number(x)?, number(y)?]),
                _ => None,
            })
            .unwrap_or_default();
        Some(NodeLayout {
            pos,
            collapsed: entry
                .get("collapsed")
                .and_then(toml::Value::as_bool)
                .unwrap_or_default(),
            label: entry.get("label").and_then(toml::Value::as_str).map(str::to_owned),
        })
    }

    /// Returns the region's canonical text, starting with its `[editor]` header.
    ///
    /// Keys follow a fixed order, `node.<id>` entries natural id order, and positions are rounded to whole points.
    pub(crate) fn to_text(&self) -> String {
        let mut text = String::from("[editor]\n");
        for name in ["version", "next_id"] {
            if let Some(value) = self.table.get(name) {
                text.push_str(&format!("{name} = {}\n", inline_value(value)));
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
            text.push_str(&format!("{} = {}\n", toml_key(name), inline_value(value)));
        }
        for name in TRAILING_TABLES {
            if let Some(items) = self.table.get(name).and_then(array_of_tables) {
                for item in items {
                    text.push_str(&format!("\n[[editor.{name}]]\n"));
                    for (key, value) in ordered_entries(item) {
                        let value = if key == "pos" { rounded(value) } else { value.clone() };
                        text.push_str(&format!("{} = {}\n", toml_key(key), inline_value(&value)));
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
            let written = if name == "node" {
                inline_value(&node_with_rounded_pos(value))
            } else {
                inline_value(value)
            };
            text.push_str(&format!("{name}.{} = {written}\n", toml_key(key)));
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
                        text.push_str(&format!(
                            "function.{}.{} = {}\n",
                            toml_key(id),
                            toml_key(key),
                            inline_value(item)
                        ));
                    }
                }
                None => {
                    text.push_str(&format!("function.{} = {}\n", toml_key(id), inline_value(value)));
                }
            }
        }
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

/// Returns `entry` with its `pos` key rounded, when it is a table that holds one.
fn node_with_rounded_pos(entry: &toml::Value) -> toml::Value {
    let mut entry = entry.clone();
    if let Some(table) = entry.as_table_mut()
        && let Some(pos) = table.get_mut("pos")
    {
        *pos = rounded(pos);
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
