//! A graph file: its document, its editor region, the text it was read from and the places of its tables.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;

use henad_core::diagnostic::CodeDiagnostic;
use henad_core::provenance::{ModelCode, ModelSource};

use crate::document::GraphDocument;
use crate::editor::EditorRegion;
use crate::limits::MAX_FILE_BYTES;
use crate::model_text::LineEnds;
use crate::reader::{self, ReadProblem};
use crate::writer;

/// Value of a model file's `format` key.
pub const FORMAT: &str = "henad-model";

/// Version of the model file format this build reads and writes.
pub const FORMAT_VERSION: i64 = 1;

/// Value of the `kind` key of a graph file.
pub const KIND_GRAPH: &str = "graph";

/// Newest node library this build reads.
pub const LIBRARY: u32 = 1;

/// Format of a graph's [`ModelCode`].
///
/// The version moves when the meaning of an unchanged file changes outside any node, such as a pass commit rule or a
/// stream derivation.
pub const CODE_FORMAT: &str = "henad-graph/1";

/// Reason a graph file cannot be read.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GraphFileError {
    /// The text is not a graph file: TOML that does not parse, a key that is unknown or missing, or a value of the
    /// wrong type.
    Toml(Box<CodeDiagnostic>),
    /// The file declares another format, version, kind or library, or a value a graph file cannot hold.
    Format(Box<CodeDiagnostic>),
}

impl GraphFileError {
    /// Diagnostic the error carries.
    pub fn diagnostic(&self) -> &CodeDiagnostic {
        match self {
            Self::Toml(diagnostic) | Self::Format(diagnostic) => diagnostic,
        }
    }
}

/// Writes the diagnostic.
impl fmt::Display for GraphFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.diagnostic())
    }
}

impl std::error::Error for GraphFileError {}

/// Places of the tables and keys of a graph file, as byte offsets into its text with line ends made `\n`.
///
/// A table's place is its header, such as `[[lane]]`. Each list matches the order of its list in the
/// [`GraphDocument`]. A place maps to the file as written through [`GraphFile::place`].
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Places {
    /// The header's `id` value.
    pub id: Range<usize>,
    /// The `[graph]` table.
    pub graph: Range<usize>,
    /// The `[[target]]` table.
    pub target: Option<Range<usize>>,
    /// The world table, `None` when the file writes none.
    pub world: Option<Range<usize>>,
    /// Each `[[category]]` table.
    pub categories: Vec<Range<usize>>,
    /// Each `[[palette]]` table.
    pub palettes: Vec<Range<usize>>,
    /// Each `[[lane]]` table.
    pub lanes: Vec<Range<usize>>,
    /// Each `[[field]]` table.
    pub fields: Vec<Range<usize>>,
    /// Each `[[counter]]` table.
    pub counters: Vec<Range<usize>>,
    /// Each `[[param]]` table.
    pub params: Vec<Range<usize>>,
    /// Each `[[stat]]` table.
    pub stats: Vec<Range<usize>>,
    /// Each `[[action]]` table.
    pub actions: Vec<Range<usize>>,
    /// Each `[[function]]` table and its nodes.
    pub functions: Vec<BodyPlaces>,
    /// Each `[[pass]]` table and its nodes, in stack order.
    pub passes: Vec<BodyPlaces>,
    /// The `[editor]` table.
    pub editor: Option<Range<usize>>,
}

impl Places {
    pub(crate) fn new(id: Range<usize>, graph: Range<usize>) -> Self {
        Self {
            id,
            graph,
            ..Self::default()
        }
    }
}

/// Places of the header of a pass or function table and its nodes.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BodyPlaces {
    /// The table's header.
    pub table: Range<usize>,
    /// Each node, in the order of [`NodeBody::nodes`](crate::node::NodeBody::nodes).
    pub nodes: Vec<NodePlaces>,
}

/// Places of one node table and its keys.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct NodePlaces {
    /// The table's header.
    pub table: Range<usize>,
    /// The `id` value.
    pub id: Range<usize>,
    /// The `op` value.
    pub op: Range<usize>,
    /// The `region` value.
    pub region: Option<Range<usize>>,
    /// Each configuration value by key.
    pub config: BTreeMap<String, Range<usize>>,
    /// Each input value by port name.
    pub inputs: BTreeMap<String, Range<usize>>,
}

/// A graph read from a model file of kind `graph`, with the text it was read from.
#[derive(Clone)]
pub struct GraphFile {
    document: GraphDocument,
    editor: Option<EditorRegion>,
    /// Text the graph was read from, or the canonical text of a graph that came from no file.
    text: String,
    /// Places in [`Self::text`] with its line ends made `\n`.
    places: Places,
    /// Map from [`Self::places`] back to [`Self::text`].
    line_ends: LineEnds,
}

/// Prints the graph's id and the byte length of the text.
impl fmt::Debug for GraphFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GraphFile")
            .field("id", &self.document.id)
            .field("passes", &self.document.passes.len())
            .field("text_bytes", &self.text.len())
            .finish_non_exhaustive()
    }
}

impl GraphFile {
    /// Reads the graph file held in `text`.
    ///
    /// A file with a duplicate node id reads, since two branches of one file can mint one id, and
    /// [`NodeBody::duplicate_ids`](crate::node::NodeBody::duplicate_ids) lists the ids of a body.
    ///
    /// # Errors
    ///
    /// Returns [`GraphFileError::Format`] for a file past [`MAX_FILE_BYTES`], which is checked before parsing, for a
    /// file of another format, version, kind or newer library, which is checked next, or for a value a graph file cannot
    /// hold, and [`GraphFileError::Toml`] for text that is not a graph file. Each diagnostic is placed in `text` when it
    /// has a place.
    pub fn parse(text: &str) -> Result<Self, GraphFileError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(GraphFileError::Format(Box::new(CodeDiagnostic::error(format!(
                "the file holds {} bytes, and a graph file holds at most {MAX_FILE_BYTES}",
                text.len()
            )))));
        }
        let (read, line_ends) = LineEnds::normalize(text);
        let place =
            |diagnostic: CodeDiagnostic, span: Range<usize>| diagnostic.at(text, line_ends.original_range(span));
        let file = reader::read(&read).map_err(|problem| match problem {
            ReadProblem::Toml(error) => {
                let diagnostic = CodeDiagnostic::error(error.message().trim_end().to_owned());
                GraphFileError::Toml(Box::new(match error.span() {
                    Some(span) => place(diagnostic, span),
                    None => diagnostic,
                }))
            }
            ReadProblem::Placed(message, span) => {
                GraphFileError::Format(Box::new(place(CodeDiagnostic::error(message), span)))
            }
        })?;
        Ok(Self {
            document: file.document,
            editor: file.editor,
            text: text.to_owned(),
            places: file.places,
            line_ends,
        })
    }

    /// Reads the graph file that `code` holds, as a results folder records it.
    ///
    /// # Errors
    ///
    /// Returns [`GraphFileError::Format`] for code in a format other than [`CODE_FORMAT`], and the errors of
    /// [`Self::parse`] for its text.
    pub fn from_code(code: &ModelCode) -> Result<Self, GraphFileError> {
        if code.format() != CODE_FORMAT {
            return Err(GraphFileError::Format(Box::new(CodeDiagnostic::error(format!(
                "Recorded under {}, and this build runs {CODE_FORMAT}",
                code.format()
            )))));
        }
        Self::parse(code.text())
    }

    /// Returns the file of `document` and `editor`, its text the canonical text.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Self::parse`] when the canonical text does not read back, as for a float that is not
    /// finite.
    pub fn from_document(document: &GraphDocument, editor: Option<&EditorRegion>) -> Result<Self, GraphFileError> {
        Self::parse(&canonical_text(document, editor))
    }

    /// Semantic document of the file.
    pub fn document(&self) -> &GraphDocument {
        &self.document
    }

    /// Editor region of the file, `None` when the file writes none.
    pub fn editor(&self) -> Option<&EditorRegion> {
        self.editor.as_ref()
    }

    /// Text the file was read from.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Places of the tables and keys of the file.
    pub fn places(&self) -> &Places {
        &self.places
    }

    /// Returns `problem` placed at `span`, a byte range of [`Self::places`], in the text the file was read from.
    pub fn place(&self, problem: CodeDiagnostic, span: Range<usize>) -> CodeDiagnostic {
        problem.at(&self.text, self.line_ends.original_range(span))
    }

    /// Returns the file's canonical text: every table and key in a fixed order and every value in one form.
    pub fn to_toml(&self) -> String {
        canonical_text(&self.document, self.editor.as_ref())
    }

    /// Returns the canonical text up to, not including, `[editor]`.
    pub fn semantic_text(&self) -> String {
        writer::semantic_text(&self.document)
    }

    /// Returns the code that identifies the graph: [`Self::semantic_text`] in the format [`CODE_FORMAT`].
    ///
    /// Presentation keys of the semantic tables, such as colours and labels, enter the hash. The editor region does not.
    pub fn code(&self) -> ModelCode {
        ModelCode::new(CODE_FORMAT, self.semantic_text())
    }

    /// Returns the source a model entry of the graph records: [`Self::code`], with the origin
    /// [`CodeOrigin::Runtime`](henad_core::provenance::CodeOrigin::Runtime), under the target's id.
    pub fn source(&self) -> ModelSource {
        ModelSource::runtime(&self.document.target.id, self.code())
    }
}

/// Returns the canonical text of `document` and `editor`.
fn canonical_text(document: &GraphDocument, editor: Option<&EditorRegion>) -> String {
    let mut text = writer::semantic_text(document);
    if let Some(editor) = editor {
        text.push('\n');
        text.push_str(&editor.to_text());
    }
    text
}
