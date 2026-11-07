//! Tests of [`crate::model_file`] on sheets: every sheet starter builds an entry under the sheet's code, a sheet's
//! errors and warnings come back placed at their tables, and a sheet is held to its size limit.

use henad_core::diagnostic::{CodeDiagnostic, DiagnosticSeverity};
use henad_core::provenance::CodeOrigin;
use henad_graph::sheet::limits::MAX_SHEET_BYTES;
use henad_graph::sheet::sentence::SHEET_EMPTY;
use henad_graph::sheet::{SheetFile, starters};

use crate::model_file::{ModelFile, ModelFileError};

/// Text of the Game of Life sheet starter.
fn life() -> &'static str {
    starters()
        .iter()
        .find(|starter| starter.file_name == "game_of_life.toml")
        .expect("the Game of Life sheet is a starter")
        .text
}

/// Returns `text` with `old` replaced by `new` once, where `text` must hold `old`.
fn edited(text: &str, old: &str, new: &str) -> String {
    assert!(text.contains(old), "the text holds no {old:?}");
    text.replacen(old, new, 1)
}

/// Returns the line of `text` that holds the first `needle`, counted from 1.
fn line_of(text: &str, needle: &str) -> u32 {
    let at = text
        .find(needle)
        .unwrap_or_else(|| panic!("the text holds no {needle:?}"));
    u32::try_from(text[..at].matches('\n').count() + 1).expect("a short text")
}

/// Returns whether `diagnostic` names no pass and no node, and mentions neither in its message or notes.
fn names_no_graph(diagnostic: &CodeDiagnostic) -> bool {
    let printed = diagnostic.to_string();
    diagnostic.pass.is_none() && diagnostic.node.is_none() && !printed.contains("node") && !printed.contains("pass")
}

#[test]
fn every_sheet_starter_reads_checks_and_builds_an_entry() {
    assert!(!starters().is_empty());
    for starter in starters() {
        let name = starter.file_name;
        let file = ModelFile::parse(starter.text).unwrap_or_else(|error| panic!("{name}: {error}"));
        let sheet = file.sheet().unwrap_or_else(|| panic!("{name} reads as a sheet"));
        assert!(file.rule().is_none());
        assert!(file.graph().is_none(), "{name}: a sheet holds no graph of its own");
        assert_eq!(file.id(), sheet.document().sheet.id);
        assert_eq!(file.to_toml(), sheet.to_toml());
        let lowered = file
            .lowered_graph()
            .unwrap_or_else(|| panic!("{name} lowers to a graph"));
        assert_eq!(lowered.document().target.id, file.id());
        assert_eq!(file.code(), sheet.code());
        assert_eq!(file.code(), lowered.code());
        assert_eq!(file.code().format(), henad_graph::CODE_FORMAT);
        assert!(file.warnings().is_empty(), "{name}: {:?}", file.warnings());
        let entry = file.entry().unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(entry.id(), file.id());
        assert_eq!(entry.source().origin(), CodeOrigin::Runtime);
        assert_eq!(entry.source().code_hash(), Some(file.code().hash()), "{name}");

        // The canonical text reads back to the same model.
        let again = ModelFile::parse(&file.to_toml()).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(again.code(), file.code());
    }
}

#[test]
fn a_sheet_error_comes_back_placed_at_its_line() {
    // A Become line that names a state the property does not hold is refused at the line's table.
    let text = edited(life(), "new_state = \"alive\"", "new_state = \"zombie\"");
    let error = ModelFile::parse(&text).expect_err("a state the property does not hold read");
    let ModelFileError::Sheet(diagnostics) = &error else {
        panic!("{error:?}");
    };
    let line = line_of(&text, "[card.line.l2]");
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.line == Some(line))
        .unwrap_or_else(|| panic!("no error at line {line}: {error}"));
    assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    assert!(diagnostic.message.contains("zombie"), "{diagnostic}");
    assert!(diagnostics.iter().all(names_no_graph), "{error}");

    // A line kind this version does not lower is refused at its line's table, in the author's words.
    let text = edited(life(), "next_line = 5", "next_line = 6");
    let text = edited(
        &text,
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l5\"]",
    );
    let text = edited(
        &text,
        "# Lines of Clear",
        "[card.line.l5]\nkind = \"repeat\"\ntimes = 2\n\n# Lines of Clear",
    );
    let error = ModelFile::parse(&text).expect_err("a Repeat read and lowered");
    let ModelFileError::Sheet(diagnostics) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(diagnostics.len(), 1, "{error}");
    assert_eq!(diagnostics[0].message, henad_graph::sheet::sentence::NOT_YET_REPEAT);
    assert_eq!(diagnostics[0].line, Some(line_of(&text, "[card.line.l5]")), "{error}");
    assert!(names_no_graph(&diagnostics[0]), "{error}");

    // A file that is no sheet is refused at its key, and a sheet version this build does not read at the version.
    let text = edited(life(), "next_line = 5", "next_line = 5\nnext_card = 2");
    let error = ModelFile::parse(&text).expect_err("an unknown key read");
    assert!(
        matches!(&error, ModelFileError::Toml(diagnostic) if diagnostic.line == Some(line_of(&text, "next_card"))),
        "{error:?}"
    );
    let text = edited(life(), "version = 1", "version = 2");
    let error = ModelFile::parse(&text).expect_err("sheet version 2 read");
    assert!(
        matches!(&error, ModelFileError::Format(diagnostic) if diagnostic.line == Some(line_of(&text, "version = 2"))),
        "{error:?}"
    );
}

#[test]
fn a_sheet_warning_comes_back_placed_at_its_line() {
    // A start with no key draws from a key the sheet derives, and the checker warns at the property.
    let text = edited(life(), "key = 2929187291\n", "");
    let file = ModelFile::parse(&text).unwrap_or_else(|error| panic!("{error}"));
    let warnings = file.warnings();
    let sheet = SheetFile::parse(&text).unwrap_or_else(|error| panic!("{error}"));
    assert!(!warnings.is_empty());
    assert_eq!(warnings, sheet.warnings());
    for warning in &warnings {
        assert_eq!(warning.severity, DiagnosticSeverity::Warning);
        assert!(warning.line.is_some() && names_no_graph(warning), "{warning}");
    }
    file.entry().unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn a_result_no_line_reads_gives_no_warning_about_the_graph() {
    // Line l1's count stays, and no line reads it once l2 and l3 compare a number with 3.
    let text = edited(life(), "left = { result = \"l1\" }\nright = 3", "left = 3\nright = 3");
    let text = edited(
        &text,
        "left = { result = \"l1\" }\nright = [2, 3]",
        "left = 3\nright = [2, 3]",
    );
    let file = ModelFile::parse(&text).unwrap_or_else(|error| panic!("{error}"));
    let graph = file.lowered_graph().expect("a sheet lowers to a graph");
    let dropped: Vec<_> = graph.check().warnings().cloned().collect();
    assert!(
        dropped
            .iter()
            .any(|warning| warning.message.contains("reaches nothing the pass keeps")),
        "{dropped:?}"
    );
    assert!(file.warnings().iter().all(names_no_graph), "{:?}", file.warnings());
    file.entry().unwrap_or_else(|error| panic!("{error}"));
}

#[test]
fn a_sheet_with_no_property_is_refused_in_its_own_words() {
    let text = &life()[..life().find("[[cells.property]]").expect("the sheet holds a property")];
    let file = ModelFile::parse(text).unwrap_or_else(|error| panic!("{error}"));
    let world = line_of(text, "[world]");
    let warnings = file.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].message, SHEET_EMPTY);
    assert_eq!(warnings[0].line, Some(world));

    // The lowered graph has no lane and no pass, and its build refuses it in the graph's words.
    let graph = file.lowered_graph().expect("a sheet lowers to a graph");
    assert!(graph.check().has_errors());
    let error = file.entry().expect_err("a sheet with no property built");
    let ModelFileError::Sheet(diagnostics) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(diagnostics.len(), 1, "{error}");
    assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Error);
    assert_eq!(diagnostics[0].message, SHEET_EMPTY);
    assert_eq!(diagnostics[0].line, Some(world), "{error}");
    assert!(
        names_no_graph(&diagnostics[0]) && !error.to_string().contains("lane"),
        "{error}"
    );
}

#[test]
fn a_sheet_is_held_to_its_size_limit() {
    let padded = format!("{}\n# {}\n", life(), "x".repeat(MAX_SHEET_BYTES));
    let error = ModelFile::parse(&padded).expect_err("a sheet past the limit read");
    let ModelFileError::Format(diagnostic) = &error else {
        panic!("{error:?}");
    };
    assert!(
        diagnostic
            .message
            .contains(&format!("a sheet holds at most {MAX_SHEET_BYTES}")),
        "{diagnostic}"
    );

    // A sheet just under the limit reads.
    let room = MAX_SHEET_BYTES - life().len() - "\n# \n".len();
    let under = format!("{}\n# {}\n", life(), "x".repeat(room));
    assert_eq!(under.len(), MAX_SHEET_BYTES);
    assert!(matches!(ModelFile::parse(&under), Ok(ModelFile::Sheet(_))));
}
