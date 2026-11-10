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

impl NodePlaces {
    /// Returns the places of a node with every key at `table`, for a node the file does not write.
    pub(crate) fn at(table: Range<usize>) -> Self {
        Self {
            id: table.clone(),
            op: table.clone(),
            region: None,
            config: BTreeMap::new(),
            inputs: BTreeMap::new(),
            table,
        }
    }
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
    /// Text of the author document the graph was lowered from, `None` for a graph read or built as a graph.
    author: Option<ModelCode>,
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
    /// file of another format, version, kind or newer library, which is checked next, for a value a graph file cannot
    /// hold, or for a file whose canonical text passes [`MAX_FILE_BYTES`], which is checked last. Returns
    /// [`GraphFileError::Toml`] for text that is not a graph file. Each diagnostic is placed in `text` when it has a
    /// place.
    pub fn parse(text: &str) -> Result<Self, GraphFileError> {
        if text.len() > MAX_FILE_BYTES {
            return Err(format_error(format!(
                "the file holds {} bytes, and a graph file holds at most {MAX_FILE_BYTES}",
                text.len()
            )));
        }
        let file = Self::read(text)?;
        // Every file this reads has a canonical text that reads back.
        let canonical = canonical_text(&file.document, file.editor.as_ref()).len();
        if canonical > MAX_FILE_BYTES {
            return Err(format_error(format!(
                "the file's canonical text holds {canonical} bytes, and a graph file holds at most {MAX_FILE_BYTES}"
            )));
        }
        Ok(file)
    }

    /// Reads the graph file held in `text` as [`Self::parse`] does, with no limit on the size of the text or of its
    /// canonical text.
    fn read(text: &str) -> Result<Self, GraphFileError> {
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
            author: None,
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
            return Err(format_error(format!(
                "Recorded under {}, and this build runs {CODE_FORMAT}",
                code.format()
            )));
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

    /// Returns the file of `document`, a normalised document, through its canonical text with no limit on its size.
    ///
    /// The checker bounds the nodes a file holds after its calls and expressions are inlined, which bounds this text
    /// too. That text can pass [`MAX_FILE_BYTES`] for a file that holds far less.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`Self::parse`] but the two of size, when the canonical text does not read back.
    pub(crate) fn from_normalised(document: &GraphDocument) -> Result<Self, GraphFileError> {
        Self::read(&canonical_text(document, None))
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

    /// Returns a placer that places many diagnostics as [`Self::place`] does, each without a scan of the text.
    pub(crate) fn placer(&self) -> Placer<'_> {
        let mut line_starts = vec![0];
        line_starts.extend(
            self.text
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(index, _)| index + 1),
        );
        Placer {
            file: self,
            line_starts,
        }
    }

    /// Returns the file's canonical text: every table and key in a fixed order and every value in one form.
    pub fn to_toml(&self) -> String {
        canonical_text(&self.document, self.editor.as_ref())
    }

    /// Runs the checker over the file: every validation rule, every failure reported at once.
    pub fn check(&self) -> crate::check::CheckReport {
        crate::check::check(self)
    }

    /// Returns the canonical text up to, not including, `[editor]`.
    pub fn semantic_text(&self) -> String {
        writer::semantic_text(&self.document)
    }

    /// Returns the code that identifies the graph: [`Self::semantic_text`] in the format [`CODE_FORMAT`].
    ///
    /// Presentation keys of the semantic tables, such as colours and labels, enter the hash.
    /// The editor region does not.
    pub fn code(&self) -> ModelCode {
        ModelCode::new(CODE_FORMAT, self.semantic_text())
    }

    /// Returns the source a model entry of the graph records: [`Self::code`], with the origin
    /// [`CodeOrigin::Runtime`](henad_core::provenance::CodeOrigin::Runtime), under the target's id, and with the
    /// [author text](Self::author) of a graph lowered from a sheet.
    pub fn source(&self) -> ModelSource {
        let source = ModelSource::runtime(&self.document.target.id, self.code());
        match &self.author {
            Some(author) => source.with_author(author.clone()),
            None => source,
        }
    }

    /// Text of the author document the graph was lowered from, such as a sheet's canonical text, and `None` for a
    /// graph read or built as a graph.
    ///
    /// The author text enters neither [`Self::code`] nor [`Self::to_toml`].
    pub fn author(&self) -> Option<&ModelCode> {
        self.author.as_ref()
    }

    /// Returns the graph with `author` recorded as the text of the author document it was lowered from.
    pub(crate) fn with_author(self, author: ModelCode) -> Self {
        Self {
            author: Some(author),
            ..self
        }
    }
}

/// Places diagnostics in the text of a [`GraphFile`], with the start of every line found once.
pub(crate) struct Placer<'a> {
    file: &'a GraphFile,
    /// Offset of the first byte of each line of the file's text.
    line_starts: Vec<usize>,
}

impl Placer<'_> {
    /// Returns `problem` placed at `span`, equal to what [`GraphFile::place`] returns.
    pub(crate) fn place(&self, problem: CodeDiagnostic, span: Range<usize>) -> CodeDiagnostic {
        let text = &self.file.text;
        let span = self.file.line_ends.original_range(span);
        let mut start = span.start.min(text.len());
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        let end = span.end.clamp(start, text.len());
        let line = self.line_starts.partition_point(|&line_start| line_start <= start);
        let line_start = self.line_starts.get(line.saturating_sub(1)).copied().unwrap_or(0);
        let column = text[line_start..start].chars().count() + 1;
        problem.with_span(
            start..end,
            u32::try_from(line).unwrap_or(u32::MAX),
            u32::try_from(column).unwrap_or(u32::MAX),
        )
    }
}

/// Returns the canonical text of `document` and `editor`.
pub(crate) fn canonical_text(document: &GraphDocument, editor: Option<&EditorRegion>) -> String {
    let mut text = writer::semantic_text(document);
    if let Some(editor) = editor {
        text.push('\n');
        text.push_str(&editor.to_text());
    }
    text
}

/// Returns a [`GraphFileError::Format`] of an error with `message` and no place.
fn format_error(message: String) -> GraphFileError {
    GraphFileError::Format(Box::new(CodeDiagnostic::error(message)))
}
