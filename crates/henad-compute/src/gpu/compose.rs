//! Composition of WGSL while the program runs, the counterpart of henad-build's composition at build time.
//!
//! [`compose_wgsl`] resolves an entry shader's `#import` lines against the shared modules of henad-core and the modules
//! a caller passes, validates the result, and writes it back as WGSL. It repeats the steps of `wgsl_bindgen` 0.23.3,
//! which henad-build runs, so a stock shader composes to its build-time `SHADER_STRING` byte for byte. `naga_oil` does
//! the composing, and this module is the only code that calls it.
//!
//! A *directive check* reads every file before `naga_oil` does. It accepts `#import` lines of one form,
//! `#import <module>::{<item>, ...}` or `#import <module>::<item>` on one line, and `#define_import_path` in a module.
//! It refuses every other `#` line, a `#`, `::` or `"` elsewhere in code, the directives `enable`, `requires` and
//! `diagnostic`, the words `virtual` and `override`, `@binding(auto)`, and `naga_oil`'s own name decoration. `naga_oil`
//! acts on each of these before naga sees the text, and one of its paths panics on a decorated name it cannot decode.
//!
//! Diagnostics point into the files as the caller passed them. `naga_oil` keeps the lines of a file but writes each
//! imported name with a longer decorated name, so a column is mapped back by aligning the tokens of the line naga read
//! with the tokens of the line the caller wrote.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

use henad_core::authoring::primitives::wgsl::SHARED_WGSL_MODULES;
use henad_core::diagnostic::CodeDiagnostic;
use naga_oil::compose::preprocess::Preprocessor;
use naga_oil::compose::tokenizer::{Token, Tokenizer};
use naga_oil::compose::{
    ComposableModuleDescriptor, Composer, ComposerError, ComposerErrorInner, ErrSource, NagaModuleDescriptor,
};

use crate::fault::catching;

/// Bit at which `naga_oil` stores a module's index in a span of the composed module, a copy of `naga_oil`'s private
/// `SPAN_SHIFT`.
///
/// Index 0 is the entry shader, and each composable module takes the next index in the order it was added. The bits
/// below hold a byte offset into the module's text with its import header in front.
pub(crate) const SPAN_SHIFT: u32 = 21;

/// Largest file the composer accepts, in bytes. A span keeps [`SPAN_SHIFT`] bits for an offset, and `naga_oil` puts an
/// import header in front of the text.
const MAX_FILE_BYTES: usize = 1 << 20;

/// Largest number of modules one composition takes. `naga_oil` numbers the modules in the bits above [`SPAN_SHIFT`].
const MAX_MODULES: usize = 1024;

/// Infix that `naga_oil` adds to the name of every item it imports.
const DECORATION: &str = "X_naga_oil_mod_X";

/// Infix that `naga_oil` adds to the name of an overriding function.
const OVERRIDE_DECORATION: &str = "X_naga_oil_vrt_X";

/// Note on a diagnostic about the text in front of the caller's own, which a grid rule's template is.
pub(crate) const BEFORE_TEXT_NOTE: &str = "the problem is in the engine's template, at no line of the rule's text";

/// A WGSL file a caller passes to [`compose_wgsl`].
#[derive(Clone, Copy)]
pub struct WgslFile<'a> {
    /// Path a diagnostic names the file by. Nothing reads the file system.
    pub path: &'a str,
    /// WGSL text of the file.
    pub text: &'a str,
}

/// Prints the path and the byte length of the text.
impl fmt::Debug for WgslFile<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WgslFile")
            .field("path", &self.path)
            .field("text_bytes", &self.text.len())
            .finish_non_exhaustive()
    }
}

/// WGSL text that [`compose_wgsl`] composed, validated and wrote back.
#[derive(Clone, PartialEq, Eq)]
pub struct ComposedWgsl {
    text: String,
}

impl ComposedWgsl {
    /// Composed text, with every import resolved. Imported names carry `naga_oil`'s decoration.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the composed text.
    pub fn into_text(self) -> String {
        self.text
    }
}

/// Prints the byte length of the text, which runs to kilobytes.
impl fmt::Debug for ComposedWgsl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComposedWgsl")
            .field("text_bytes", &self.text.len())
            .finish_non_exhaustive()
    }
}

/// Composes `entry` against the shared modules and `modules`, validates the result, and writes it back as WGSL.
///
/// The shared modules of henad-core are added first, in `SHARED_WGSL_MODULES` order, then each of `modules`, which
/// declare their own `#define_import_path` and come in dependency order. The module is validated with every flag and
/// every capability and written with naga's WGSL writer and no flags, the steps `wgsl_bindgen` 0.23.3 takes for an
/// embedded shader.
///
/// # Errors
///
/// Returns one diagnostic per problem the directive check finds, or the one problem `naga_oil` or naga reports. A
/// diagnostic names the file by its [`WgslFile::path`], and its span, line and column point into the text the caller
/// passed. A problem inside a shared module, or inside the import header `naga_oil` writes, has no place.
pub fn compose_wgsl(entry: WgslFile<'_>, modules: &[WgslFile<'_>]) -> Result<ComposedWgsl, Vec<CodeDiagnostic>> {
    let composition = compose(entry, modules, 0)?;
    let text = composition.write()?;
    Ok(ComposedWgsl { text })
}

/// A composed and validated naga module, with what it takes to place a span of the entry in the entry's text.
pub(crate) struct Composition {
    /// Composed module.
    pub(crate) module: naga::Module,
    /// Validation result of [`Self::module`], with every flag and every capability.
    pub(crate) info: naga::valid::ModuleInfo,
    /// The entry as the caller wrote it.
    entry: FileInfo,
    /// The entry as `naga_oil` handed it to naga, after its own preprocessing and before the import header.
    preprocessed: SourceLines,
}

impl Composition {
    /// Returns the module written back as WGSL.
    ///
    /// # Errors
    ///
    /// Returns the writer's error as one diagnostic with no place.
    pub(crate) fn write(&self) -> Result<String, Vec<CodeDiagnostic>> {
        naga::back::wgsl::write_string(&self.module, &self.info, naga::back::wgsl::WriterFlags::empty()).map_err(
            |error| {
                vec![CodeDiagnostic::error(format!(
                    "the composed shader could not be written: {error}"
                ))]
            },
        )
    }

    /// Returns the byte offset at which the entry's text starts in the spans of [`Self::module`].
    ///
    /// `naga_oil` parses the entry behind an import header whose length it keeps to itself. The offset is read back from
    /// `anchor`, a function of the entry with no attribute, whose `fn` keyword starts at byte `anchor_offset` of the
    /// entry's text and on a line that `naga_oil` leaves as it is. `None` when the anchor is missing or its line changed.
    pub(crate) fn entry_start_offset(&self, anchor: &str, anchor_offset: usize) -> Option<usize> {
        let (handle, _) = self
            .module
            .functions
            .iter()
            .find(|(_, function)| function.name.as_deref() == Some(anchor))?;
        let (index, range) = split_span(self.module.functions.get_span(handle))?;
        if index != 0 {
            return None;
        }
        let line = self.entry.lines.line_of(anchor_offset)?;
        let column = anchor_offset - self.entry.lines.lines[line].start;
        let preprocessed = self.preprocessed.lines.get(line)?;
        if preprocessed.text != self.entry.lines.lines[line].text {
            return None;
        }
        range.start.checked_sub(preprocessed.start + column)
    }

    /// Returns the place in the caller's text of `span`, a span of [`Self::module`] in the entry, given the
    /// `start_offset` that [`Self::entry_start_offset`] returned.
    ///
    /// `None` for a span of an imported module, of the import header, of the text in front of the caller's, or with
    /// no place.
    pub(crate) fn locate_in_entry(&self, span: naga::Span, start_offset: usize) -> Option<Located> {
        let (index, range) = split_span(span)?;
        if index != 0 {
            return None;
        }
        let start = range.start.checked_sub(start_offset)?;
        let end = range.end.checked_sub(start_offset)?;
        let mapped = map_range(&self.preprocessed, &self.entry.lines, start..end)?;
        self.entry.place(mapped)
    }
}

/// A place in a file: its bytes, and the line and character column where they start, both counted from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Located {
    /// Bytes of the file.
    pub(crate) span: Range<usize>,
    /// Line of the first byte.
    pub(crate) line: u32,
    /// Column of the first byte, in characters.
    pub(crate) column: u32,
}

impl Located {
    /// Returns `diagnostic` placed here.
    pub(crate) fn place(&self, diagnostic: CodeDiagnostic) -> CodeDiagnostic {
        diagnostic.with_span(self.span.clone(), self.line, self.column)
    }
}

/// Returns a span's module index and its byte range within that module, or `None` for a span with no place.
pub(crate) fn split_span(span: naga::Span) -> Option<(usize, Range<usize>)> {
    let range = span.to_range()?;
    let mask = (1usize << SPAN_SHIFT) - 1;
    let index = range.start >> SPAN_SHIFT;
    Some((index, (range.start & mask)..(range.end & mask)))
}

/// One file as the composer sees it: its path, its lines, the import path it declares, and where the caller's own
/// text starts in it.
struct FileInfo {
    path: String,
    lines: SourceLines,
    /// Import path from the file's `#define_import_path`, `None` for the entry.
    import_path: Option<String>,
    /// Byte offset of the caller's text, at the start of a line. Text in front of it is reported without a place.
    base: usize,
    /// Number of lines in front of [`Self::base`].
    base_lines: u32,
}

impl FileInfo {
    fn new(path: &str, text: &str, import_path: Option<String>, base: usize) -> Self {
        let lines = SourceLines::new(text);
        let base_lines = lines.line_of(base).map_or(0, to_u32);
        Self {
            path: path.to_owned(),
            lines,
            import_path,
            base,
            base_lines,
        }
    }

    /// Returns the place of `range`, a byte range of the whole file, in the caller's text, or `None` when it starts
    /// in front of it.
    fn place(&self, range: Range<usize>) -> Option<Located> {
        let start = range.start.checked_sub(self.base)?;
        let end = range.end.saturating_sub(self.base).max(start);
        let (line, column) = self.lines.line_column(range.start);
        Some(Located {
            span: start..end,
            line: line - self.base_lines,
            column,
        })
    }
}

/// Returns `entry` composed against the shared modules and `modules`, and validated.
///
/// The caller's own text starts at byte `base` of the entry, at the start of a line. Diagnostics point into that text,
/// and a problem in front of it has no place.
///
/// # Errors
///
/// Returns the problems of the directive check, or the one problem `naga_oil` reports, or the validation error.
pub(crate) fn compose(
    entry: WgslFile<'_>,
    modules: &[WgslFile<'_>],
    base: usize,
) -> Result<Composition, Vec<CodeDiagnostic>> {
    if modules.len() > MAX_MODULES {
        return Err(vec![CodeDiagnostic::error(format!(
            "{} modules passed, and a composition takes at most {MAX_MODULES}",
            modules.len()
        ))]);
    }
    let mut known: Vec<String> = SHARED_WGSL_MODULES
        .iter()
        .map(|module| module.import_path.to_owned())
        .collect();
    // The import paths of the modules are read first, so an entry or a module can name any of them.
    let declared: Vec<Option<String>> = modules.iter().map(|module| declared_import_path(module.text)).collect();
    known.extend(declared.iter().flatten().cloned());

    let mut files = Vec::with_capacity(modules.len() + 1);
    files.push(FileInfo::new(entry.path, entry.text, None, base));
    for (module, import_path) in modules.iter().zip(declared) {
        files.push(FileInfo::new(module.path, module.text, import_path, 0));
    }
    let mut problems = check_directives(entry.text, false, &known, &files[0]);
    for (module, file) in modules.iter().zip(&files[1..]) {
        problems.extend(check_directives(module.text, true, &known, file));
    }
    if !problems.is_empty() {
        return Err(problems);
    }

    // The directive check stops the text that panics naga_oil. The catch is a backstop on native. On the web, built
    // with `panic_abort`, a panic ends the page.
    let composed = catching("composing WGSL", || compose_checked(entry, modules, &files))
        .unwrap_or_else(|fault| Err(vec![CodeDiagnostic::error(format!("{COMPOSER_FAILED}: {fault}"))]))?;
    let (module, info) = composed;
    let preprocessed = SourceLines::new(&preprocess_entry(entry.text));
    let entry_file = files.swap_remove(0);
    Ok(Composition {
        module,
        info,
        entry: entry_file,
        preprocessed,
    })
}

/// Start of the message of a diagnostic about a panic inside `naga_oil`.
pub(crate) const COMPOSER_FAILED: &str = "the WGSL composer failed on this text";

/// Composes files that passed the directive check, and validates the module for the writer.
fn compose_checked(
    entry: WgslFile<'_>,
    modules: &[WgslFile<'_>],
    files: &[FileInfo],
) -> Result<(naga::Module, naga::valid::ModuleInfo), Vec<CodeDiagnostic>> {
    let mut composer = Composer::default();
    let descriptors = SHARED_WGSL_MODULES
        .iter()
        .map(|shared| ComposableModuleDescriptor {
            source: shared.source,
            file_path: shared.import_path,
            as_name: Some(shared.import_path.to_owned()),
            ..Default::default()
        })
        .chain(modules.iter().map(|module| ComposableModuleDescriptor {
            source: module.text,
            file_path: module.path,
            ..Default::default()
        }));
    for descriptor in descriptors {
        if let Err(error) = composer.add_composable_module(descriptor).map(|_| ()) {
            return Err(vec![diagnose(&composer, &error, files)]);
        }
    }
    let module = composer
        .make_naga_module(NagaModuleDescriptor {
            source: entry.text,
            file_path: entry.path,
            ..Default::default()
        })
        .map_err(|error| vec![diagnose(&composer, &error, files)])?;
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .map_err(|error| vec![CodeDiagnostic::error(undecorate(&error.as_inner().to_string()))])?;
    Ok((module, info))
}

/// Returns the entry's text as `naga_oil` preprocesses it before it parses the text behind the import header.
///
/// The steps are those of `naga_oil`'s `make_naga_module` for a file the directive check passed. Line ends are made
/// `\n` with one at the end, the WGSL directives are taken out (the check leaves none), and the preprocessor runs
/// with no shader definitions.
fn preprocess_entry(text: &str) -> String {
    let mut sanitized = text.replace("\r\n", "\n").replace('\r', "\n");
    if !sanitized.ends_with('\n') {
        sanitized.push('\n');
    }
    let preprocessor = Preprocessor::default();
    let Ok(metadata) = preprocessor.get_preprocessor_metadata(&sanitized, true) else {
        return String::new();
    };
    match preprocessor.preprocess(&metadata.cleaned_source, &HashMap::new()) {
        Ok(output) => output.preprocessed_source,
        Err(_) => String::new(),
    }
}

/// Returns the import path that `text` declares with `#define_import_path`, or `None` when it declares none.
fn declared_import_path(text: &str) -> Option<String> {
    code_lines(text).into_iter().find_map(|line| {
        let rest = line.code.trim().strip_prefix("#define_import_path")?;
        let path = rest.trim();
        is_module_path(path).then(|| path.to_owned())
    })
}

/// Returns whether `path` is one or more identifiers joined by `::`.
fn is_module_path(path: &str) -> bool {
    !path.is_empty() && path.split("::").all(is_identifier)
}

/// Returns whether `word` is an ASCII WGSL identifier.
pub(crate) fn is_identifier(word: &str) -> bool {
    let mut chars = word.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && word != "_"
        && !word.starts_with("__")
}

/// A line of a file with its comments blanked out.
#[derive(Debug)]
pub(crate) struct CodeLine {
    /// Byte offset of the line in the file.
    pub(crate) start: usize,
    /// The line with each byte of a comment replaced by a space, so a byte offset into it is one into the line.
    pub(crate) code: String,
}

/// Returns the lines of `text` with comments blanked out, split at `\r\n`, a lone `\r` or `\n`, as `naga_oil` splits
/// them.
///
/// Block comments nest, as in WGSL and in `naga_oil`'s own comment stripping.
pub(crate) fn code_lines(text: &str) -> Vec<CodeLine> {
    let mut lines = Vec::new();
    let mut depth = 0usize;
    for (start, line) in split_lines(text) {
        let bytes = line.as_bytes();
        let mut code = String::with_capacity(line.len());
        let mut index = 0;
        while index < bytes.len() {
            let pair = bytes.get(index..index + 2);
            if depth > 0 {
                if pair == Some(b"/*") {
                    depth += 1;
                    code.push_str("  ");
                    index += 2;
                } else if pair == Some(b"*/") {
                    depth -= 1;
                    code.push_str("  ");
                    index += 2;
                } else {
                    code.push(' ');
                    index += 1;
                }
            } else if pair == Some(b"//") {
                code.extend(std::iter::repeat_n(' ', bytes.len() - index));
                index = bytes.len();
            } else if pair == Some(b"/*") {
                depth = 1;
                code.push_str("  ");
                index += 2;
            } else {
                // One whole character, so the blanked line keeps the line's byte offsets.
                let width = line[index..].chars().next().map_or(1, char::len_utf8);
                code.push_str(&line[index..index + width]);
                index += width;
            }
        }
        lines.push(CodeLine { start, code });
    }
    lines
}

/// Returns each line of `text` with its byte offset, split at `\r\n`, a lone `\r` or `\n`.
fn split_lines(text: &str) -> Vec<(usize, &str)> {
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\n' => {
                lines.push((start, &text[start..index]));
                index += 1;
                start = index;
            }
            b'\r' => {
                lines.push((start, &text[start..index]));
                index += if bytes.get(index + 1) == Some(&b'\n') { 2 } else { 1 };
                start = index;
            }
            _ => index += 1,
        }
    }
    if start < text.len() {
        lines.push((start, &text[start..]));
    }
    lines
}

/// Returns the problems the directive check finds in `text`, the file `file` describes.
///
/// `is_module` allows the file's `#define_import_path`. `known` lists every import path a file can import from.
fn check_directives(text: &str, is_module: bool, known: &[String], file: &FileInfo) -> Vec<CodeDiagnostic> {
    if text.len() > MAX_FILE_BYTES {
        return vec![
            CodeDiagnostic::error(format!(
                "the file holds {} bytes, and the composer takes at most {MAX_FILE_BYTES}",
                text.len()
            ))
            .with_file(&file.path),
        ];
    }
    let mut problems = Vec::new();
    let mut problem = |line: &CodeLine, column: usize, length: usize, message: String| {
        let start = line.start + column;
        let diagnostic = CodeDiagnostic::error(message).with_file(&file.path);
        problems.push(match file.place(start..start + length) {
            Some(located) => located.place(diagnostic),
            None => diagnostic.with_note(BEFORE_TEXT_NOTE),
        });
    };
    for (line, (_, raw)) in code_lines(text).iter().zip(split_lines(text)) {
        if let Some(column) = raw.find(DECORATION).or_else(|| raw.find(OVERRIDE_DECORATION)) {
            problem(
                line,
                column,
                DECORATION.len(),
                "the text holds the name decoration the composer adds to imported items".to_owned(),
            );
            continue;
        }
        let trimmed = line.code.trim_start();
        let indent = line.code.len() - trimmed.len();
        if trimmed.starts_with('#') {
            if let Err(message) = check_directive_line(trimmed, is_module, known) {
                problem(line, indent, trimmed.trim_end().len(), message);
            }
            continue;
        }
        if let Some(word) = leading_word(trimmed)
            && matches!(word, "enable" | "requires" | "diagnostic")
        {
            problem(
                line,
                indent,
                word.len(),
                format!("the `{word}` directive is not supported"),
            );
            continue;
        }
        if let Some(column) = line.code.find('#') {
            problem(
                line,
                column,
                1,
                "`#` is allowed only at the start of an `#import` line".to_owned(),
            );
        } else if let Some(column) = line.code.find("::") {
            problem(
                line,
                column,
                2,
                "WGSL has no `::`. Import an item with `#import <module>::{<item>}` and name it alone".to_owned(),
            );
        } else if let Some(column) = line.code.find('"') {
            problem(line, column, 1, "WGSL has no strings".to_owned());
        } else if let Some(column) = line.code.find("@binding(auto)") {
            problem(
                line,
                column,
                "@binding(auto)".len(),
                "a binding needs a number".to_owned(),
            );
        } else if let Some((column, word)) = words(&line.code).find(|(_, word)| matches!(*word, "virtual" | "override"))
        {
            problem(line, column, word.len(), format!("`{word}` is not supported"));
        }
    }
    problems
}

/// Returns the first identifier of `text` when `text` starts with one.
fn leading_word(text: &str) -> Option<&str> {
    let end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    (end > 0).then(|| &text[..end])
}

/// Returns every word of `code` made of letters, digits and underscores, with its byte offset.
pub(crate) fn words(code: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut start = None;
    let mut found = Vec::new();
    for (index, c) in code.char_indices().chain(std::iter::once((code.len(), ' '))) {
        let in_word = c.is_alphanumeric() || c == '_';
        match (start, in_word) {
            (None, true) => start = Some(index),
            (Some(begin), false) => {
                found.push((begin, &code[begin..index]));
                start = None;
            }
            _ => {}
        }
    }
    found.into_iter()
}

/// Checks one line whose code starts with `#`, and returns the problem's message.
fn check_directive_line(trimmed: &str, is_module: bool, known: &[String]) -> Result<(), String> {
    let code = trimmed.trim_end();
    if let Some(rest) = code.strip_prefix("#define_import_path") {
        if !is_module {
            return Err("an entry shader declares no `#define_import_path`".to_owned());
        }
        return if rest.starts_with(char::is_whitespace) && is_module_path(rest.trim()) {
            Ok(())
        } else {
            Err("write `#define_import_path <module>`, with a module path of identifiers".to_owned())
        };
    }
    let Some(rest) = code.strip_prefix("#import") else {
        return Err("only `#import` lines are allowed here. naga_oil's other directives are not supported".to_owned());
    };
    if !rest.starts_with(char::is_whitespace) {
        return Err("write `#import <module>::{<item>, ...}`".to_owned());
    }
    let body = rest.trim().trim_end_matches(';').trim_end();
    let (module, items) = match body.split_once("::{") {
        Some((module, list)) => {
            let Some(list) = list.strip_suffix('}') else {
                return Err("an `#import` line closes its `{` on the same line".to_owned());
            };
            let items: Vec<&str> = list.split(',').map(str::trim).filter(|item| !item.is_empty()).collect();
            (module, items)
        }
        None => match body.rsplit_once("::") {
            Some((module, item)) => (module, vec![item.trim()]),
            None => return Err("write `#import <module>::{<item>, ...}`".to_owned()),
        },
    };
    if !is_module_path(module) {
        return Err(format!("`{module}` is not a module path"));
    }
    if !known.iter().any(|path| path == module) {
        return Err(format!("there is no module `{module}` to import from"));
    }
    if items.is_empty() {
        return Err("an `#import` line names at least one item".to_owned());
    }
    for item in items {
        let valid = match item.split_once(" as ") {
            Some((name, alias)) => is_identifier(name.trim()) && is_identifier(alias.trim()),
            None => is_identifier(item),
        };
        if !valid {
            return Err(format!("`{item}` is not an item name"));
        }
    }
    Ok(())
}

/// Lines of a text, each with its byte offset and its content without the line end.
#[derive(Debug, Clone)]
pub(crate) struct SourceLines {
    lines: Vec<SourceLine>,
    /// Byte length of the whole text.
    len: usize,
}

#[derive(Debug, Clone)]
struct SourceLine {
    start: usize,
    text: String,
}

impl SourceLines {
    /// Returns the lines of `text`, split at `\r\n`, a lone `\r` or `\n`.
    pub(crate) fn new(text: &str) -> Self {
        Self {
            lines: split_lines(text)
                .into_iter()
                .map(|(start, line)| SourceLine {
                    start,
                    text: line.to_owned(),
                })
                .collect(),
            len: text.len(),
        }
    }

    /// Returns the index of the line holding byte `offset`, the last line for an offset past the end.
    fn line_of(&self, offset: usize) -> Option<usize> {
        if self.lines.is_empty() {
            return None;
        }
        let after = self.lines.partition_point(|line| line.start <= offset);
        Some(after.saturating_sub(1))
    }

    /// Returns the line and the character column, both from 1, of byte `offset`.
    pub(crate) fn line_column(&self, offset: usize) -> (u32, u32) {
        let Some(index) = self.line_of(offset) else {
            return (1, 1);
        };
        let line = &self.lines[index];
        let within = offset.saturating_sub(line.start).min(line.text.len());
        let within = floor_char_boundary(&line.text, within);
        let column = line.text[..within].chars().count() + 1;
        (to_u32(index + 1), to_u32(column))
    }
}

/// Returns `value` as a `u32`, saturating.
fn to_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Returns the largest character boundary of `text` at or below `index`.
fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Returns `range`, a byte range in `preprocessed`, as the byte range of `original` it came from. `preprocessed` is
/// `original` after `naga_oil`'s preprocessing.
///
/// `naga_oil` keeps every line, and replaces each imported name in a line with its decorated name. A position keeps its
/// line, and its column is mapped by aligning the tokens of the two lines.
fn map_range(preprocessed: &SourceLines, original: &SourceLines, range: Range<usize>) -> Option<Range<usize>> {
    let start = map_offset(preprocessed, original, range.start)?;
    let end = map_offset(preprocessed, original, range.end)
        .unwrap_or(start)
        .max(start);
    Some(start..end)
}

/// Returns the byte offset in `original` that byte `offset` of `preprocessed` came from.
fn map_offset(preprocessed: &SourceLines, original: &SourceLines, offset: usize) -> Option<usize> {
    if offset > preprocessed.len {
        return None;
    }
    let index = preprocessed.line_of(offset)?;
    let line = &preprocessed.lines[index];
    // An offset past the last line, as at the end of the text, lands at the end of the last original line.
    let Some(source) = original.lines.get(index) else {
        let last = original.lines.last()?;
        return Some(last.start + last.text.len());
    };
    if offset > line.start + line.text.len() {
        return Some(source.start + source.text.len());
    }
    let column = map_column(&line.text, &source.text, offset - line.start);
    Some(source.start + column)
}

/// Returns the byte column in `source` that byte column `column` of `preprocessed` came from.
///
/// The two lines hold the same tokens apart from imported names, which `preprocessed` writes decorated. Within a
/// changed name the column is clamped to the original name.
fn map_column(preprocessed: &str, source: &str, column: usize) -> usize {
    if preprocessed == source {
        return floor_char_boundary(source, column);
    }
    let theirs: Vec<Token<'_>> = Tokenizer::new(preprocessed, true).collect();
    let ours: Vec<Token<'_>> = Tokenizer::new(source, true).collect();
    for (their, our) in theirs.iter().zip(&ours) {
        let their_end = their.pos() + token_len(their);
        if column < their_end {
            let within = (column - their.pos()).min(token_len(our));
            return floor_char_boundary(source, our.pos() + within);
        }
    }
    source.len()
}

/// Returns the byte length of `token`.
fn token_len(token: &Token<'_>) -> usize {
    match token {
        Token::Identifier(text, _) | Token::Whitespace(text, _) => text.len(),
        Token::Other(c, _) => c.len_utf8(),
    }
}

/// Returns `text` with `naga_oil`'s decoration taken off every imported name, so a message names the item as written.
pub(crate) fn undecorate(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = [DECORATION, OVERRIDE_DECORATION]
        .iter()
        .filter_map(|marker| rest.find(marker))
        .min()
    {
        out.push_str(&rest[..index]);
        let after = &rest[index + DECORATION.len()..];
        let encoded = after
            .find(|c: char| !(c.is_ascii_uppercase() || c.is_ascii_digit()))
            .unwrap_or(after.len());
        rest = after[encoded..].strip_prefix('X').unwrap_or(&after[encoded..]);
    }
    out.push_str(rest);
    out
}

/// Returns the diagnostic for an error that `naga_oil` returned while it composed `files`.
///
/// One label that lands in the caller's text places the diagnostic, and every other label becomes a note. A parse
/// error is placed by its first such label. A validation error lists its spans from the outermost, the function, to
/// the innermost, the expression at fault, and is placed by its last.
fn diagnose(composer: &Composer, error: &ComposerError, files: &[FileInfo]) -> CodeDiagnostic {
    let mut diagnostic = CodeDiagnostic::error(undecorate(&error.inner.to_string()));
    // The file the error is about, and the module index its validation spans carry.
    let found = match &error.source {
        ErrSource::Constructing { path, .. } => files
            .iter()
            .position(|file| file.path == *path)
            .map(|position| (position, 0)),
        ErrSource::Module { name, .. } => files
            .iter()
            .position(|file| file.import_path.as_deref() == Some(name.as_str()))
            // The shared modules come first, then the caller's modules, each one index after the last.
            .map(|position| (position, position + SHARED_WGSL_MODULES.len())),
    };
    let (labels, notes) = labels_and_notes(&error.inner);
    let Some((position, module_index)) = found else {
        // An error inside a shared module is one of Henad's, and has no place in the caller's files.
        let name = match &error.source {
            ErrSource::Constructing { path, .. } => path,
            ErrSource::Module { name, .. } => name,
        };
        diagnostic = diagnostic.with_note(format!("in {name}"));
        return with_notes(diagnostic, labels.into_iter().map(|(_, label)| label).chain(notes));
    };
    let file = &files[position];
    diagnostic = diagnostic.with_file(&file.path);
    let preprocessed = SourceLines::new(&error.source.source(composer));
    let offset = error.source.offset();
    let mut in_front = false;
    let located: Vec<(Option<Located>, String)> = labels
        .into_iter()
        .map(|(span, label)| {
            let range = span.and_then(|(index, range)| {
                // Validation spans carry a module index. Parse and import positions carry none.
                if index != 0 && index != module_index {
                    return None;
                }
                let start = range.start.checked_sub(offset)?;
                let end = range.end.checked_sub(offset).unwrap_or(start);
                map_range(&preprocessed, &file.lines, start..end)
            });
            let located = range.as_ref().and_then(|range| file.place(range.clone()));
            in_front |= range.is_some() && located.is_none();
            (located, label)
        })
        .collect();
    let validation = matches!(
        error.inner,
        ComposerErrorInner::ShaderValidationError(_) | ComposerErrorInner::HeaderValidationError(_)
    );
    let mut placed_at = located.iter().map(|(located, _)| located.is_some());
    let chosen = if validation {
        placed_at.rposition(|placed| placed)
    } else {
        placed_at.position(|placed| placed)
    };
    let mut rest = Vec::with_capacity(located.len());
    for (index, (place, label)) in located.into_iter().enumerate() {
        match place {
            Some(place) if Some(index) == chosen => {
                diagnostic = place.place(diagnostic);
                rest.insert(0, label);
            }
            _ => rest.push(label),
        }
    }
    if chosen.is_none() && in_front {
        diagnostic = diagnostic.with_note(BEFORE_TEXT_NOTE);
    }
    with_notes(diagnostic, rest.into_iter().chain(notes))
}

/// Returns `diagnostic` with each non-empty text of `notes` added as a note, undecorated.
fn with_notes(diagnostic: CodeDiagnostic, notes: impl Iterator<Item = String>) -> CodeDiagnostic {
    notes
        .filter(|note| !note.is_empty())
        .fold(diagnostic, |diagnostic, note| diagnostic.with_note(undecorate(&note)))
}

/// A label's span as a module index and a byte range, or `None` when the label has no place.
type LabelSpan = Option<(usize, Range<usize>)>;

/// Returns the labelled spans of an error, the first the primary, and its further notes.
fn labels_and_notes(inner: &ComposerErrorInner) -> (Vec<(LabelSpan, String)>, Vec<String>) {
    let at = |position: usize| Some((0, position..position));
    match inner {
        ComposerErrorInner::WgslParseError(error) => (
            error
                .labels()
                .map(|(span, label)| (split_span(span), label.to_owned()))
                .collect(),
            error.notes().map(str::to_owned).collect(),
        ),
        ComposerErrorInner::ShaderValidationError(error) | ComposerErrorInner::HeaderValidationError(error) => {
            let mut notes = Vec::new();
            let mut source = std::error::Error::source(error);
            while let Some(cause) = source {
                notes.push(cause.to_string());
                source = cause.source();
            }
            (
                error
                    .spans()
                    .map(|(span, label)| (split_span(*span), label.clone()))
                    .collect(),
                notes,
            )
        }
        ComposerErrorInner::ImportParseError(_, position)
        | ComposerErrorInner::ImportNotFound(_, position)
        | ComposerErrorInner::NotEnoughEndIfs(position)
        | ComposerErrorInner::TooManyEndIfs(position)
        | ComposerErrorInner::ElseWithoutCondition(position)
        | ComposerErrorInner::DefineInModule(position)
        | ComposerErrorInner::UnknownShaderDef { pos: position, .. }
        | ComposerErrorInner::UnknownShaderDefOperator { pos: position, .. }
        | ComposerErrorInner::InvalidShaderDefComparisonValue { pos: position, .. }
        | ComposerErrorInner::OverrideNotVirtual { pos: position, .. }
        | ComposerErrorInner::InvalidShaderDefDefinitionValue { pos: position, .. } => {
            (vec![(at(*position), String::new())], Vec::new())
        }
        ComposerErrorInner::DecorationInSource(range) => (vec![(Some((0, range.clone())), String::new())], Vec::new()),
        ComposerErrorInner::InvalidIdentifier { at: span, .. } => {
            (vec![(split_span(*span), String::new())], Vec::new())
        }
        _ => (Vec::new(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::{SourceLines, WgslFile, code_lines, compose_wgsl, map_column, undecorate};

    #[test]
    fn a_comment_is_blanked_byte_for_byte() {
        let lines = code_lines("let a = 1; // é note\n/* x /* y */ z */ let b = 2;\n");
        assert_eq!(lines[0].code, "let a = 1;           ");
        assert_eq!(lines[0].code.len(), "let a = 1; // é note".len());
        assert_eq!(lines[1].code.trim(), "let b = 2;");
    }

    #[test]
    fn a_decorated_name_maps_back_to_the_name_as_written() {
        let source = "let r = next_bits(rng) + 1u;";
        let preprocessed = "let r = next_bitsX_naga_oil_mod_XNBSW4YLEHI5HE3THX(rng) + 1u;";
        let plus = preprocessed.find('+').unwrap_or_default();
        assert_eq!(
            map_column(preprocessed, source, plus),
            source.find('+').unwrap_or_default()
        );
        // A column inside the decoration is clamped to the end of the name.
        assert_eq!(map_column(preprocessed, source, 20), "let r = next_bits".len());
    }

    #[test]
    fn undecorate_strips_the_module_suffix() {
        assert_eq!(
            undecorate("no definition for `next_bitsX_naga_oil_mod_XNBSW4YLEHI5HE3THX`"),
            "no definition for `next_bits`"
        );
    }

    #[test]
    fn a_column_counts_characters() {
        let lines = SourceLines::new("ab\r\nüé x\n");
        assert_eq!(lines.line_column("ab\r\nüé ".len()), (2, 4));
    }

    #[test]
    fn foreign_directives_are_refused_at_their_line() {
        let entry = "#ifdef FOO\nfn a() {}\n#endif\nfn b() -> u32 { return henad::rng::pcg_hash(1u); }\n";
        let problems = compose_wgsl(
            WgslFile {
                path: "e.wgsl",
                text: entry,
            },
            &[],
        )
        .err()
        .unwrap_or_default();
        let lines: Vec<Option<u32>> = problems.iter().map(|problem| problem.line).collect();
        assert_eq!(lines, [Some(1), Some(3), Some(4)], "{problems:#?}");
    }
}
