//! Reader of a graph file's text into a [`GraphDocument`], with the place of every table and key.
//!
//! The reader checks what the document's own types need: the shape of every table, the grammars of node ids, port
//! names and type strings, the literal forms and finite floats. Rules about what a graph means are left to a checker
//! that reads the document and its [`Places`].

use std::collections::BTreeMap;
use std::ops::Range;
use std::str::FromStr;

use serde::de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use toml::Spanned;

use henad_core::authoring::primitives::space::Boundary;
use henad_core::metadata::TimeScale;
use henad_core::params::ParamDescriptor;

use crate::document::{
    Action, AgentsWorld, Category, Counter, Domain, Field, FieldCombine, Function, FunctionPort, GraphDocument,
    GridWorld, Lane, LaneRole, LaneStorage, Palette, Pass, PassEnabled, PassStage, Stat, Target, Variant, World,
};
use crate::editor::EditorRegion;
use crate::file::{BodyPlaces, NodePlaces, Places};
use crate::ids::is_port_segment;
use crate::model_text::{ModelHeader, ParamTable};
use crate::node::{Node, NodeBody, OP_FUNCTION_CALL};
use crate::types::ValueType;
use crate::value::{Color, ConfigValue, InputValue, Literal, MemberName, PortRef};

/// Problem that stops a read.
pub(crate) enum ReadProblem {
    /// TOML that does not parse, a key that is unknown or missing, or a value of the wrong type.
    Toml(toml::de::Error),
    /// A value a graph file cannot hold, with its bytes in the text.
    Placed(String, Range<usize>),
}

impl From<toml::de::Error> for ReadProblem {
    fn from(error: toml::de::Error) -> Self {
        Self::Toml(error)
    }
}

impl From<Placed> for ReadProblem {
    fn from(problem: Placed) -> Self {
        Self::Placed(problem.0, problem.1)
    }
}

/// Message and bytes of a value the reader refuses.
struct Placed(String, Range<usize>);

/// Result of a read.
pub(crate) struct ReadFile {
    pub(crate) document: GraphDocument,
    pub(crate) editor: Option<EditorRegion>,
    pub(crate) places: Places,
}

/// Reads `text`, whose line ends are already `\n`.
pub(crate) fn read(text: &str) -> Result<ReadFile, ReadProblem> {
    // The header first, so that a file of another version reads as one even when its keys are new.
    let header = ModelHeader::read(text)?;
    header
        .check(&[crate::file::KIND_GRAPH])
        .map_err(|problem| Placed(problem.message, problem.span))?;
    check_library(&header)?;
    let file: FileTable = toml::from_str(text)?;
    Ok(file.into_read_file()?)
}

/// Checks the `[graph] library` key read with the header, and returns it when this build cannot read it.
fn check_library(header: &ModelHeader) -> Result<(), Placed> {
    let Some(library) = header.graph_library() else {
        return Ok(());
    };
    let number = *library.get_ref();
    if number < 1 {
        return Err(Placed(
            format!("the file names node library {number}, but libraries count from 1"),
            library.span(),
        ));
    }
    if number > i64::from(crate::file::LIBRARY) {
        return Err(Placed(
            format!(
                "the file uses node library {number}, and this build reads up to {}",
                crate::file::LIBRARY
            ),
            library.span(),
        ));
    }
    Ok(())
}

/// A graph file as TOML holds it. [`ModelHeader`] checks the header's values.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileTable {
    #[serde(rename = "format")]
    _format: IgnoredAny,
    #[serde(rename = "format_version")]
    _format_version: IgnoredAny,
    #[serde(rename = "kind")]
    _kind: IgnoredAny,
    id: Spanned<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: String,
    graph: Spanned<GraphTable>,
    #[serde(default, rename = "target")]
    targets: Vec<Spanned<TargetTable>>,
    #[serde(default)]
    grid: Option<Spanned<GridTable>>,
    #[serde(default)]
    agents: Option<Spanned<AgentsTable>>,
    #[serde(default, rename = "category")]
    categories: Vec<Spanned<CategoryTable>>,
    #[serde(default, rename = "palette")]
    palettes: Vec<Spanned<PaletteTable>>,
    #[serde(default, rename = "lane")]
    lanes: Vec<Spanned<LaneTable>>,
    #[serde(default, rename = "field")]
    fields: Vec<Spanned<FieldTable>>,
    #[serde(default, rename = "counter")]
    counters: Vec<Spanned<CounterTable>>,
    #[serde(default, rename = "param")]
    params: Vec<Spanned<ParamTable>>,
    #[serde(default, rename = "stat")]
    stats: Vec<Spanned<StatTable>>,
    #[serde(default, rename = "action")]
    actions: Vec<Spanned<ActionTable>>,
    #[serde(default, rename = "function")]
    functions: Vec<Spanned<FunctionTable>>,
    #[serde(default, rename = "pass")]
    passes: Vec<Spanned<PassTable>>,
    #[serde(default)]
    editor: Option<Spanned<toml::Table>>,
}

/// The `[graph]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphTable {
    topology: TopologyName,
    library: u32,
}

/// Value of the `topology` key.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum TopologyName {
    Grid,
    Agents,
}

/// A `[[target]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetTable {
    backend: Spanned<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// The `[grid]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GridTable {
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    boundary: Option<BoundaryName>,
}

/// Value of the `boundary` key.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum BoundaryName {
    Torus,
    Bounded,
}

/// The `[agents]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentsTable {
    #[serde(default)]
    count: Option<u32>,
    #[serde(default)]
    max_count: Option<u32>,
    #[serde(default)]
    width: Option<Spanned<RawValue>>,
    #[serde(default)]
    height: Option<Spanned<RawValue>>,
}

/// A `[[category]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CategoryTable {
    id: String,
    variants: Vec<Spanned<VariantTable>>,
}

/// One element of a category's `variants`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VariantTable {
    name: String,
    #[serde(default)]
    label: Option<String>,
    color: Spanned<Vec<u8>>,
}

/// A `[[palette]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PaletteTable {
    id: String,
    colors: Vec<Spanned<Vec<u8>>>,
}

/// A `[[lane]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LaneTable {
    id: String,
    domain: DomainName,
    #[serde(rename = "type")]
    value_type: Spanned<String>,
    #[serde(default)]
    storage: Option<Spanned<StorageName>>,
    #[serde(default)]
    initial: Option<Spanned<RawValue>>,
    #[serde(default)]
    role: Option<RoleName>,
}

/// Value of a `domain` key.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum DomainName {
    Cells,
    Agents,
}

impl DomainName {
    fn into_domain(self) -> Domain {
        match self {
            Self::Cells => Domain::Cells,
            Self::Agents => Domain::Agents,
        }
    }
}

/// Value of a lane's `storage` key.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum StorageName {
    U8,
    U16,
    U32,
}

/// Value of a lane's `role` key.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum RoleName {
    Position,
}

/// A `[[field]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldTable {
    id: String,
    #[serde(rename = "type")]
    value_type: Spanned<String>,
    #[serde(default)]
    layers: Option<Vec<String>>,
    #[serde(default, rename = "static")]
    is_static: bool,
    #[serde(default)]
    combine: Option<FieldCombineName>,
    #[serde(default)]
    resolution: Option<Spanned<f64>>,
    #[serde(default)]
    finish: Option<String>,
    #[serde(default)]
    initial: Option<Spanned<RawValue>>,
}

/// Value of a field's `combine` key.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum FieldCombineName {
    Max,
    Sum,
}

/// A `[[counter]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CounterTable {
    id: String,
}

/// A `[[stat]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatTable {
    label: String,
    from: Spanned<String>,
    color: Spanned<Vec<u8>>,
}

/// An `[[action]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionTable {
    id: String,
    label: String,
    #[serde(default)]
    runs: Vec<String>,
}

/// A `[[function]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionTable {
    id: Spanned<String>,
    #[serde(rename = "kind")]
    _kind: FunctionKindName,
    #[serde(default, rename = "input")]
    inputs: Vec<FunctionPortTable>,
    #[serde(default, rename = "output")]
    outputs: Vec<FunctionPortTable>,
    #[serde(default, rename = "node")]
    nodes: Vec<Spanned<NodeTable>>,
}

/// Value of a function's `kind` key, of which library 1 has one.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum FunctionKindName {
    Pure,
}

/// A `[[function.input]]` or `[[function.output]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionPortTable {
    port: String,
    #[serde(rename = "type")]
    value_type: Spanned<String>,
}

/// A `[[pass]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PassTable {
    id: Spanned<String>,
    stage: StageName,
    #[serde(default)]
    action: Option<String>,
    #[serde(rename = "kind")]
    _kind: PassKindName,
    #[serde(default)]
    domain: Option<DomainName>,
    #[serde(default)]
    enabled: Option<Spanned<RawValue>>,
    #[serde(default)]
    draw_order: Vec<Spanned<String>>,
    #[serde(default, rename = "node")]
    nodes: Vec<Spanned<NodeTable>>,
}

/// Value of a pass's `stage` key.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum StageName {
    Init,
    Tick,
    View,
    Stats,
    Action,
}

/// Value of a pass's `kind` key, of which graph models have one.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum PassKindName {
    Map,
}

/// A `[[pass.node]]` or `[[function.node]]` table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NodeTable {
    id: Spanned<String>,
    op: Spanned<String>,
    #[serde(default)]
    config: Option<Spanned<RawValue>>,
    #[serde(default, rename = "in")]
    inputs: Option<Spanned<RawValue>>,
    #[serde(default)]
    region: Option<Spanned<String>>,
}

/// A TOML value with the bytes of everything inside it, for the keys whose schema depends on a node's kind.
///
/// A datetime reaches [`RawValue::Table`] under a private key of the TOML crate, which no grammar accepts.
enum RawValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Array(Vec<Spanned<Self>>),
    Table(Vec<(Spanned<String>, Spanned<Self>)>),
}

impl<'de> Deserialize<'de> for RawValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(RawVisitor)
    }
}

/// Visitor of [`RawValue`].
struct RawVisitor;

impl<'de> Visitor<'de> for RawVisitor {
    type Value = RawValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a TOML value")
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<RawValue, E> {
        Ok(RawValue::Boolean(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<RawValue, E> {
        Ok(RawValue::Integer(value))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<RawValue, E> {
        i64::try_from(value)
            .map(RawValue::Integer)
            .map_err(|_error| E::custom("the integer is out of range"))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<RawValue, E> {
        Ok(RawValue::Float(value))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<RawValue, E> {
        Ok(RawValue::String(value.to_owned()))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<RawValue, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(RawValue::Array(items))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawValue, A::Error> {
        let mut entries = Vec::new();
        while let Some(key) = map.next_key()? {
            entries.push((key, map.next_value()?));
        }
        Ok(RawValue::Table(entries))
    }
}

impl FileTable {
    /// Returns the document, the editor region and the places the tables hold.
    fn into_read_file(self) -> Result<ReadFile, Placed> {
        let mut places = Places::new(self.id.span(), self.graph.span());
        let id = self.id.into_inner();
        let name = self.name.unwrap_or_else(|| id.clone());
        let target = read_target(
            self.targets,
            &id,
            &name,
            &self.description,
            &mut places,
            self.graph.span(),
        )?;
        let graph = self.graph.into_inner();
        let world = read_world(graph.topology, self.grid, self.agents, &mut places)?;

        let mut categories = Vec::with_capacity(self.categories.len());
        for table in self.categories {
            places.categories.push(table.span());
            categories.push(read_category(table.into_inner())?);
        }
        let mut palettes = Vec::with_capacity(self.palettes.len());
        for table in self.palettes {
            places.palettes.push(table.span());
            palettes.push(read_palette(table.into_inner())?);
        }
        let mut lanes = Vec::with_capacity(self.lanes.len());
        for table in self.lanes {
            places.lanes.push(table.span());
            lanes.push(read_lane(table.into_inner(), &categories)?);
        }
        let mut fields = Vec::with_capacity(self.fields.len());
        for table in self.fields {
            let span = table.span();
            places.fields.push(span.clone());
            fields.push(read_field(table.into_inner(), span, &categories)?);
        }
        let mut counters = Vec::with_capacity(self.counters.len());
        for table in self.counters {
            places.counters.push(table.span());
            counters.push(Counter {
                id: table.into_inner().id,
            });
        }
        let (params, time) = read_params(self.params, &mut places)?;
        let mut stats = Vec::with_capacity(self.stats.len());
        for table in self.stats {
            places.stats.push(table.span());
            stats.push(read_stat(table.into_inner())?);
        }
        let mut actions = Vec::with_capacity(self.actions.len());
        for table in self.actions {
            places.actions.push(table.span());
            let ActionTable { id, label, runs } = table.into_inner();
            actions.push(Action { id, label, runs });
        }
        let mut functions = Vec::with_capacity(self.functions.len());
        for table in self.functions {
            let (function, body_places) = read_function(table)?;
            functions.push(function);
            places.functions.push(body_places);
        }
        let (passes, pass_places) = read_passes(self.passes)?;
        places.passes = pass_places;
        places.editor = self.editor.as_ref().map(Spanned::span);

        let document = GraphDocument {
            id,
            name,
            description: self.description,
            library: graph.library,
            target,
            world,
            categories,
            palettes,
            lanes,
            fields,
            counters,
            params,
            time,
            stats,
            actions,
            functions,
            passes,
        };
        Ok(ReadFile {
            document,
            editor: self.editor.map(|table| EditorRegion::from_table(table.into_inner())),
            places,
        })
    }
}

/// Returns the file's one target.
fn read_target(
    targets: Vec<Spanned<TargetTable>>,
    id: &str,
    name: &str,
    description: &str,
    places: &mut Places,
    graph: Range<usize>,
) -> Result<Target, Placed> {
    let mut tables = targets.into_iter();
    let Some(first) = tables.next() else {
        return Err(Placed(
            "the file needs one `[[target]]` table with `backend = \"gpu\"`".to_owned(),
            graph,
        ));
    };
    if let Some(second) = tables.next() {
        return Err(Placed(
            "the file holds a second `[[target]]` table, and a file holds one".to_owned(),
            second.span(),
        ));
    }
    places.target = Some(first.span());
    let first = first.into_inner();
    if first.backend.get_ref() != "gpu" {
        return Err(Placed(
            format!(
                "the target's backend is '{}', but graph models compile to WGSL, so it must be 'gpu'",
                first.backend.get_ref()
            ),
            first.backend.span(),
        ));
    }
    Ok(Target {
        id: first.id.unwrap_or_else(|| id.to_owned()),
        name: first.name.unwrap_or_else(|| name.to_owned()),
        description: first.description.unwrap_or_else(|| description.to_owned()),
    })
}

/// Returns the world table of `topology`, with its defaults when the file writes none.
fn read_world(
    topology: TopologyName,
    grid: Option<Spanned<GridTable>>,
    agents: Option<Spanned<AgentsTable>>,
    places: &mut Places,
) -> Result<World, Placed> {
    match topology {
        TopologyName::Grid => {
            if let Some(agents) = agents {
                return Err(Placed(
                    "an `[agents]` table does not belong to the grid topology".to_owned(),
                    agents.span(),
                ));
            }
            let mut world = GridWorld::default();
            if let Some(table) = grid {
                places.world = Some(table.span());
                let table = table.into_inner();
                world.width = table.width.unwrap_or(world.width);
                world.height = table.height.unwrap_or(world.height);
                world.boundary = match table.boundary {
                    Some(BoundaryName::Torus) | None => Boundary::Torus,
                    Some(BoundaryName::Bounded) => Boundary::Bounded,
                };
            }
            Ok(World::Grid(world))
        }
        TopologyName::Agents => {
            if let Some(grid) = grid {
                return Err(Placed(
                    "a `[grid]` table does not belong to the agents topology".to_owned(),
                    grid.span(),
                ));
            }
            let mut world = AgentsWorld::default();
            if let Some(table) = agents {
                places.world = Some(table.span());
                let table = table.into_inner();
                world.count = table.count.unwrap_or(world.count);
                world.max_count = table.max_count.unwrap_or(world.max_count);
                if let Some(width) = &table.width {
                    world.width = read_f32(width)?;
                }
                if let Some(height) = &table.height {
                    world.height = read_f32(height)?;
                }
            }
            Ok(World::Agents(world))
        }
    }
}

/// Returns the colour `bytes` holds.
fn read_color(bytes: &Spanned<Vec<u8>>) -> Result<Color, Placed> {
    match bytes.get_ref()[..] {
        [red, green, blue] => Ok([red, green, blue, 255]),
        [red, green, blue, alpha] => Ok([red, green, blue, alpha]),
        _ => Err(Placed(
            format!("a colour has 3 or 4 bytes, and this one has {}", bytes.get_ref().len()),
            bytes.span(),
        )),
    }
}

fn read_category(table: CategoryTable) -> Result<Category, Placed> {
    let mut variants = Vec::with_capacity(table.variants.len());
    for variant in table.variants {
        let variant = variant.into_inner();
        variants.push(Variant {
            color: read_color(&variant.color)?,
            label: variant.label.unwrap_or_else(|| variant.name.clone()),
            name: variant.name,
        });
    }
    Ok(Category { id: table.id, variants })
}

fn read_palette(table: PaletteTable) -> Result<Palette, Placed> {
    Ok(Palette {
        id: table.id,
        colors: table.colors.iter().map(read_color).collect::<Result<_, _>>()?,
    })
}

/// Returns the value `text` reads as, such as the type a type string declares, placed at `text` when it does not
/// read.
fn parsed<T: FromStr<Err = String>>(text: &Spanned<String>) -> Result<T, Placed> {
    text.get_ref().parse().map_err(|message| Placed(message, text.span()))
}

/// Returns the default value of a lane or field of type `value_type`: its zero, or the first variant of its category.
fn default_initial(value_type: &ValueType, categories: &[Category]) -> Option<Literal> {
    if let ValueType::Category(id) = value_type {
        let category = categories.iter().find(|category| category.id == *id)?;
        return category.variants.first().map(|variant| {
            Literal::Category(MemberName {
                owner: id.clone(),
                member: variant.name.clone(),
            })
        });
    }
    Literal::zero_of(value_type)
}

fn read_lane(table: LaneTable, categories: &[Category]) -> Result<Lane, Placed> {
    let value_type = parsed(&table.value_type)?;
    let storage = match (&value_type, table.storage) {
        (ValueType::U32 | ValueType::Category(_), Some(storage)) => Some(match storage.into_inner() {
            StorageName::U8 => LaneStorage::U8,
            StorageName::U16 => LaneStorage::U16,
            StorageName::U32 => LaneStorage::U32,
        }),
        (ValueType::U32, None) => Some(LaneStorage::U32),
        (ValueType::Category(_), None) => Some(LaneStorage::U8),
        (_, Some(storage)) => {
            return Err(Placed(
                format!(
                    "lane '{}' of type '{value_type}' must not set `storage`, which belongs to `u32` and category lanes",
                    table.id
                ),
                storage.span(),
            ));
        }
        (_, None) => None,
    };
    let initial = match &table.initial {
        Some(raw) => Some(read_literal(raw)?),
        None => default_initial(&value_type, categories),
    };
    Ok(Lane {
        id: table.id,
        domain: table.domain.into_domain(),
        value_type,
        storage,
        initial,
        role: table.role.map(|role| match role {
            RoleName::Position => LaneRole::Position,
        }),
    })
}

/// Reads the `[[field]]` table `table`, which spans `span` of the file.
fn read_field(table: FieldTable, span: Range<usize>, categories: &[Category]) -> Result<Field, Placed> {
    let value_type = parsed(&table.value_type)?;
    let initial = match &table.initial {
        Some(raw) => Some(read_literal(raw)?),
        None => default_initial(&value_type, categories),
    };
    let combine = match (table.combine, table.resolution) {
        (None, None) => None,
        (Some(FieldCombineName::Max), None) => Some(FieldCombine::Max),
        (Some(FieldCombineName::Sum), Some(resolution)) => {
            Some(FieldCombine::sum(*resolution.get_ref()).map_err(|message| Placed(message, resolution.span()))?)
        }
        (Some(FieldCombineName::Sum), None) => {
            return Err(Placed(
                format!(
                    "the field '{}' adds up its deposits, and needs a `resolution`",
                    table.id
                ),
                span,
            ));
        }
        (_, Some(resolution)) => {
            return Err(Placed(
                format!(
                    "the field '{}' has a `resolution`, and only a field with `combine = \"sum\"` takes one",
                    table.id
                ),
                resolution.span(),
            ));
        }
    };
    Ok(Field {
        id: table.id,
        value_type,
        layers: table.layers.unwrap_or_else(|| vec!["value".to_owned()]),
        is_static: table.is_static,
        combine,
        finish: table.finish,
        initial,
    })
}

/// Returns the parameters `tables` declare, and the time scale of the one whose table sets `tick_unit`.
///
/// A second table that sets `tick_unit` is refused at its place, since a file has one tick length.
fn read_params(
    tables: Vec<Spanned<ParamTable>>,
    places: &mut Places,
) -> Result<(Vec<ParamDescriptor>, Option<TimeScale>), Placed> {
    let mut params = Vec::with_capacity(tables.len());
    let mut time: Option<TimeScale> = None;
    for table in tables {
        let span = table.span();
        places.params.push(span.clone());
        let (param, unit) = table
            .into_inner()
            .into_graph_param()
            .map_err(|message| Placed(message, span.clone()))?;
        if let Some(unit) = unit {
            if let Some(first) = &time {
                let message = format!(
                    "the parameters '{}' and '{}' both set `tick_unit`, and a file has one tick length",
                    first.tick_length,
                    param.id()
                );
                return Err(Placed(message, span));
            }
            time = Some(TimeScale::new(unit, param.id().to_owned()));
        }
        params.push(param);
    }
    Ok((params, time))
}

fn read_stat(table: StatTable) -> Result<Stat, Placed> {
    Ok(Stat {
        from: parsed(&table.from)?,
        color: read_color(&table.color)?,
        label: table.label,
    })
}

fn read_function(table: Spanned<FunctionTable>) -> Result<(Function, BodyPlaces), Placed> {
    let span = table.span();
    let table = table.into_inner();
    let read_ports = |ports: Vec<FunctionPortTable>| -> Result<Vec<FunctionPort>, Placed> {
        ports
            .into_iter()
            .map(|port| {
                Ok(FunctionPort {
                    value_type: parsed(&port.value_type)?,
                    port: port.port,
                })
            })
            .collect()
    };
    let inputs = read_ports(table.inputs)?;
    let outputs = read_ports(table.outputs)?;
    let (body, nodes) = read_body(table.nodes)?;
    Ok((
        Function {
            id: table.id.into_inner(),
            inputs,
            outputs,
            body,
        },
        BodyPlaces { table: span, nodes },
    ))
}

/// Returns the passes in stack order, with their places in the same order.
fn read_passes(tables: Vec<Spanned<PassTable>>) -> Result<(Vec<Pass>, Vec<BodyPlaces>), Placed> {
    let mut read = Vec::with_capacity(tables.len());
    for table in tables {
        read.push(read_pass(table)?);
    }
    // A stable sort keeps file order within a stage.
    read.sort_by_key(|(pass, _)| pass.stage);
    Ok(read.into_iter().unzip())
}

fn read_pass(table: Spanned<PassTable>) -> Result<(Pass, BodyPlaces), Placed> {
    let span = table.span();
    let table = table.into_inner();
    let enabled = match &table.enabled {
        None => PassEnabled::Literal(true),
        Some(raw) => match raw.get_ref() {
            RawValue::Boolean(value) => PassEnabled::Literal(*value),
            RawValue::Table(entries) => match entries.as_slice() {
                [(key, value)] if key.get_ref() == "param" => match value.get_ref() {
                    RawValue::String(id) => PassEnabled::Param(id.clone()),
                    _ => return Err(not_enabled(raw.span())),
                },
                _ => return Err(not_enabled(raw.span())),
            },
            _ => return Err(not_enabled(raw.span())),
        },
    };
    let draw_order = table.draw_order.iter().map(parsed).collect::<Result<_, _>>()?;
    let (body, nodes) = read_body(table.nodes)?;
    Ok((
        Pass {
            id: table.id.into_inner(),
            stage: match table.stage {
                StageName::Init => PassStage::Init,
                StageName::Tick => PassStage::Tick,
                StageName::View => PassStage::View,
                StageName::Stats => PassStage::Stats,
                StageName::Action => PassStage::Action,
            },
            action: table.action,
            domain: table.domain.map(DomainName::into_domain),
            enabled,
            draw_order,
            body,
        },
        BodyPlaces { table: span, nodes },
    ))
}

/// Returns the refusal of a pass's `enabled` key.
fn not_enabled(span: Range<usize>) -> Placed {
    Placed(
        "`enabled` is `true`, `false` or `{ param = \"<bool parameter>\" }`".to_owned(),
        span,
    )
}

/// Returns the body the tables hold, its nodes in natural id order with their places in the same order.
fn read_body(tables: Vec<Spanned<NodeTable>>) -> Result<(NodeBody, Vec<NodePlaces>), Placed> {
    let mut read = Vec::with_capacity(tables.len());
    for table in tables {
        read.push(read_node(table)?);
    }
    // A stable sort keeps the file order of a duplicate id.
    read.sort_by_key(|(node, _)| node.id);
    let (nodes, places) = read.into_iter().unzip();
    Ok((NodeBody { nodes }, places))
}

fn read_node(table: Spanned<NodeTable>) -> Result<(Node, NodePlaces), Placed> {
    let span = table.span();
    let table = table.into_inner();
    let mut node = Node::new(parsed(&table.id)?, table.op.get_ref().clone());
    let mut places = NodePlaces {
        table: span,
        id: table.id.span(),
        op: table.op.span(),
        region: table.region.as_ref().map(Spanned::span),
        config: BTreeMap::new(),
        inputs: BTreeMap::new(),
    };
    if let Some(config) = &table.config {
        read_config(config, &mut node, &mut places)?;
    }
    if let Some(inputs) = &table.inputs {
        read_inputs(inputs, &mut node, &mut places)?;
    }
    node.region = table.region.as_ref().map(parsed).transpose()?;
    Ok((node, places))
}

/// Reads the `config` table of a node.
fn read_config(config: &Spanned<RawValue>, node: &mut Node, places: &mut NodePlaces) -> Result<(), Placed> {
    let RawValue::Table(entries) = config.get_ref() else {
        return Err(Placed("`config` is a table of keys".to_owned(), config.span()));
    };
    for (key, value) in entries {
        check_segment(key)?;
        let read = read_config_value(value)?;
        if matches!(read, ConfigValue::Table(_)) {
            return Err(Placed(
                format!(
                    "`config.{}` is a table, but a configuration value is a scalar or an array",
                    key.get_ref()
                ),
                value.span(),
            ));
        }
        node.config.insert(key.get_ref().clone(), read);
        places.config.insert(key.get_ref().clone(), value.span());
    }
    Ok(())
}

fn read_config_value(value: &Spanned<RawValue>) -> Result<ConfigValue, Placed> {
    Ok(match value.get_ref() {
        RawValue::String(text) => ConfigValue::String(text.clone()),
        RawValue::Integer(number) => ConfigValue::Integer(*number),
        RawValue::Float(number) => {
            if !number.is_finite() {
                return Err(not_finite(value.span()));
            }
            ConfigValue::Float(*number)
        }
        RawValue::Boolean(flag) => ConfigValue::Bool(*flag),
        RawValue::Array(items) => ConfigValue::Array(items.iter().map(read_config_value).collect::<Result<_, _>>()?),
        RawValue::Table(entries) => {
            let mut table = BTreeMap::new();
            for (key, item) in entries {
                check_segment(key)?;
                table.insert(key.get_ref().clone(), read_config_value(item)?);
            }
            ConfigValue::Table(table)
        }
    })
}

/// Checks that `key` follows the port segment grammar, which lets the writer leave it bare.
fn check_segment(key: &Spanned<String>) -> Result<(), Placed> {
    if is_port_segment(key.get_ref()) {
        Ok(())
    } else {
        Err(Placed(
            format!(
                "'{}' is no key of a node, whose keys are letters, digits and `_`",
                key.get_ref()
            ),
            key.span(),
        ))
    }
}

/// Returns the number of segments below `prefix` in the name of a port that configuration makes.
///
/// A region's `init.<acc>` and a category switch's `case.<variant>` have one, and a transition's `rule.<id>.when` and
/// `rule.<id>.to` have two. No fixed port has one of these names.
fn config_port_depth(prefix: &str) -> usize {
    match prefix {
        "init" | "case" => 1,
        "rule" => 2,
        _ => 0,
    }
}

/// Reads the `in` table of a node.
///
/// TOML stores a dotted input as nested tables. A prefix of a port that configuration makes selects how far the walk
/// descends, and the value there is an input value, even a one-key table such as `{ param = "n5" }`. A function call's
/// interface ports are all bare, so a port of a call named `init` holds a value.
fn read_inputs(inputs: &Spanned<RawValue>, node: &mut Node, places: &mut NodePlaces) -> Result<(), Placed> {
    let RawValue::Table(entries) = inputs.get_ref() else {
        return Err(Placed("`in` is a table of ports".to_owned(), inputs.span()));
    };
    let bare = node.op == OP_FUNCTION_CALL || node.op == crate::node::OP_EXPRESSION;
    for (key, value) in entries {
        check_segment(key)?;
        let depth = if bare { 0 } else { config_port_depth(key.get_ref()) };
        read_input_below(key.get_ref(), value, depth, node, places)?;
    }
    Ok(())
}

/// Reads the input or inputs below the port name `port`, with `depth` more segments to walk.
fn read_input_below(
    port: &str,
    value: &Spanned<RawValue>,
    depth: usize,
    node: &mut Node,
    places: &mut NodePlaces,
) -> Result<(), Placed> {
    if depth == 0 {
        node.inputs.insert(port.to_owned(), read_input_value(value)?);
        places.inputs.insert(port.to_owned(), value.span());
        return Ok(());
    }
    let RawValue::Table(entries) = value.get_ref() else {
        return Err(Placed(
            format!("input `in.{port}` needs a name after it, as in `in.{port}.<name>`"),
            value.span(),
        ));
    };
    for (key, item) in entries {
        check_segment(key)?;
        read_input_below(&format!("{port}.{}", key.get_ref()), item, depth - 1, node, places)?;
    }
    Ok(())
}

fn read_input_value(value: &Spanned<RawValue>) -> Result<InputValue, Placed> {
    let refused = || {
        Placed(
            "an input is a link such as \"n3\", a literal such as `{ f32 = 1.0 }` or a binding such as `{ param = \"rate\" }`"
                .to_owned(),
            value.span(),
        )
    };
    match value.get_ref() {
        RawValue::String(text) => text
            .parse::<PortRef>()
            .map(InputValue::Link)
            .map_err(|message| Placed(message, value.span())),
        RawValue::Table(entries) => match entries.as_slice() {
            [(key, bound)] if key.get_ref() == "param" => match bound.get_ref() {
                RawValue::String(id) => Ok(InputValue::Param(id.clone())),
                _ => Err(refused()),
            },
            [_] => read_literal(value).map(InputValue::Literal),
            _ => Err(refused()),
        },
        _ => Err(refused()),
    }
}

/// Returns the number `value` holds, which an integer may spell.
fn read_number(value: &Spanned<RawValue>) -> Result<f64, Placed> {
    let number = match value.get_ref() {
        RawValue::Float(number) => *number,
        RawValue::Integer(number) => *number as f64,
        _ => return Err(Placed("this value needs to be a number".to_owned(), value.span())),
    };
    if number.is_finite() {
        Ok(number)
    } else {
        Err(not_finite(value.span()))
    }
}

fn read_f32(value: &Spanned<RawValue>) -> Result<f32, Placed> {
    let number = read_number(value)? as f32;
    if number.is_finite() {
        Ok(number)
    } else {
        Err(Placed("this value is too large for an `f32`".to_owned(), value.span()))
    }
}

/// Returns the whole number `value` holds, narrowed to `T`.
fn read_integer<T: TryFrom<i64>>(value: &Spanned<RawValue>, type_name: &str) -> Result<T, Placed> {
    let RawValue::Integer(number) = value.get_ref() else {
        return Err(Placed(
            format!("this value needs to be a whole `{type_name}`"),
            value.span(),
        ));
    };
    T::try_from(*number).map_err(|_error| Placed(format!("{number} is out of range for `{type_name}`"), value.span()))
}

/// Returns the two components of the array `value` holds.
fn read_pair<T>(
    value: &Spanned<RawValue>,
    component: impl Fn(&Spanned<RawValue>) -> Result<T, Placed>,
) -> Result<[T; 2], Placed> {
    match value.get_ref() {
        RawValue::Array(items) => match items.as_slice() {
            [first, second] => Ok([component(first)?, component(second)?]),
            _ => Err(Placed(
                format!("a vector has 2 components, and this one has {}", items.len()),
                value.span(),
            )),
        },
        _ => Err(Placed("a vector is an array of 2 components".to_owned(), value.span())),
    }
}

/// Returns the `u64` a string holds, in decimal or `0x` hexadecimal.
pub(crate) fn read_u64(text: &str) -> Option<u64> {
    let (digits, radix) = match text.strip_prefix("0x") {
        Some(digits) => (digits, 16),
        None => (text, 10),
    };
    // `from_str_radix` also takes a leading `+`.
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    u64::from_str_radix(digits, radix).ok()
}

/// Returns the literal a one-key table holds.
fn read_literal(table: &Spanned<RawValue>) -> Result<Literal, Placed> {
    let (tag, value) = match table.get_ref() {
        RawValue::Table(entries) => match entries.as_slice() {
            [(tag, value)] => (tag.get_ref().as_str(), value),
            _ => return Err(not_literal(table.span())),
        },
        _ => return Err(not_literal(table.span())),
    };
    let member = || -> Result<MemberName, Placed> {
        if let RawValue::String(text) = value.get_ref()
            && let Some((owner, member)) = text.split_once('.')
            && !owner.is_empty()
            && !member.is_empty()
        {
            return Ok(MemberName {
                owner: owner.to_owned(),
                member: member.to_owned(),
            });
        }
        Err(Placed(
            format!("a `{tag}` literal is a string `<id>.<name>`"),
            value.span(),
        ))
    };
    match tag {
        "bool" => match value.get_ref() {
            RawValue::Boolean(flag) => Ok(Literal::Bool(*flag)),
            _ => Err(Placed("a `bool` literal is `true` or `false`".to_owned(), value.span())),
        },
        "u32" => read_integer(value, "u32").map(Literal::U32),
        "i32" => read_integer(value, "i32").map(Literal::I32),
        "f32" => read_f32(value).map(Literal::F32),
        "f64" => read_number(value).map(Literal::F64),
        "u64" => match value.get_ref() {
            RawValue::String(text) => read_u64(text).map(Literal::U64).ok_or_else(|| {
                Placed(
                    format!("'{text}' is no `u64`, written in decimal or as `0x` hexadecimal"),
                    value.span(),
                )
            }),
            _ => Err(Placed(
                "a `u64` literal is a string, since TOML integers are signed".to_owned(),
                value.span(),
            )),
        },
        "vec2u" => read_pair(value, |item| read_integer(item, "u32")).map(Literal::Vec2U),
        "vec2i" => read_pair(value, |item| read_integer(item, "i32")).map(Literal::Vec2I),
        "vec2f" => read_pair(value, read_f32).map(Literal::Vec2F),
        "cat" => member().map(Literal::Category),
        "layer" => member().map(Literal::Layer),
        "option" => member().map(Literal::ChoiceOption),
        _ => Err(not_literal(table.span())),
    }
}

/// Returns the refusal of a float that is not finite.
fn not_finite(span: Range<usize>) -> Placed {
    Placed("a float in a graph file must be finite".to_owned(), span)
}

/// Returns the refusal of a value that is no literal.
fn not_literal(span: Range<usize>) -> Placed {
    Placed(
        "a literal is a one-key table tagged by its type, such as `{ f32 = 1.0 }` or `{ cat = \"health.I\" }`"
            .to_owned(),
        span,
    )
}
