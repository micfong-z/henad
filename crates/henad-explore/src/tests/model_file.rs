//! Tests of [`crate::model_file`]: reading, writing, the canonical text and the places of diagnostics.

use std::path::Path;

use henad_compute::gpu::runtime_grid::{GridRule, LIFE_STEP, RuleState};
use henad_core::authoring::primitives::space::Boundary;
use henad_core::helpers::{bool_param, choice_param, u32_param};
use henad_core::provenance::{CodeOrigin, ModelCode};

use crate::model_file::{ModelFile, ModelFileError};

/// Returns the Life starter with a parameter of every kind, a third state, a bounded grid and a step that holds
/// non-ASCII text.
fn rich_rule() -> GridRule {
    let mut rule = GridRule::life();
    rule.description = "Life with \"quotes\", a back\\slash and é".to_owned();
    rule.params.push(u32_param("radius", "Radius", 2, 1, 5));
    rule.params.push(bool_param("wrap", "Wrap", true));
    rule.params.push(choice_param("mode", "Mode", &["calm", "wild"], 1));
    rule.params[0] = rule.params[0].clone().percent();
    rule.states.push(RuleState::new("Ghost", [90, 90, 255, 128]));
    rule.boundary = Boundary::Bounded;
    rule.width = 64;
    rule.height = 48;
    rule.step = format!("// État « vivant »\n{LIFE_STEP}");
    rule
}

/// Returns the problems in `error` as `line:column: message` lines.
fn lines_of(error: &ModelFileError) -> Vec<String> {
    error.diagnostics().iter().map(ToString::to_string).collect()
}

#[test]
fn a_model_file_round_trips_through_to_toml() {
    let file = ModelFile::from_rule(rich_rule()).unwrap_or_else(|error| panic!("{error}"));
    let text = file.to_toml();
    let read = ModelFile::parse(&text).unwrap_or_else(|error| panic!("{error}\n{text}"));
    assert_eq!(read.to_toml(), text);
    assert_eq!(read.code(), file.code());
    let rule = read.rule();
    assert_eq!(rule.step, rich_rule().step);
    assert_eq!(rule.description, rich_rule().description);
    assert_eq!(rule.states, rich_rule().states);
    assert_eq!(format!("{:?}", rule.params), format!("{:?}", rich_rule().params));
    assert_eq!((rule.boundary, rule.width, rule.height), (Boundary::Bounded, 64, 48));

    // A results folder holds the code, and the code gives the file back.
    let again = ModelFile::from_code(&file.code()).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(again.to_toml(), text);
    assert!(matches!(
        ModelFile::from_code(&ModelCode::new("henad-grid-rule/0", text.clone())),
        Err(ModelFileError::Format(_))
    ));

    // A step a literal string cannot hold is refused.
    let mut rule = GridRule::life();
    rule.step.push_str("// '''\n");
    assert!(matches!(ModelFile::from_rule(rule), Err(ModelFileError::Format(_))));

    // So is a step in a basic string that holds `'''`, which the canonical text could not write back, at its line.
    let basic = text
        .replace("wgsl = '''\n", "wgsl = \"\"\"\n")
        .replace("}\n'''\n", "}\n// '''\n\"\"\"\n");
    assert_ne!(basic, text);
    let error = ModelFile::parse(&basic)
        .err()
        .unwrap_or_else(|| panic!("a step holding `'''` was read:\n{basic}"));
    let line = basic[..basic.find("// '''").unwrap_or_default()].matches('\n').count() as u32 + 1;
    assert!(
        matches!(&error, ModelFileError::Format(diagnostic) if diagnostic.line == Some(line)),
        "{error:?}"
    );

    // A choice whose options text could not select apart is refused at its table.
    let repeated = text.replace("options = [\"calm\", \"wild\"]", "options = [\"calm\", \"calm\"]");
    assert_ne!(repeated, text);
    let error = ModelFile::parse(&repeated)
        .err()
        .unwrap_or_else(|| panic!("a repeated option was read"));
    assert!(error.to_string().contains("option 'calm' appears twice"), "{error}");

    // So is an option that the level text of a sweep could not list.
    let every = text.replace("options = [\"calm\", \"wild\"]", "options = [\"all\", \"some\"]");
    assert_ne!(every, text);
    let error = ModelFile::parse(&every)
        .err()
        .unwrap_or_else(|| panic!("an option named 'all' was read"));
    assert!(
        error
            .to_string()
            .contains("option 'all' cannot be listed in a sweep's levels"),
        "{error}"
    );
}

#[test]
fn unknown_keys_are_refused() {
    let text = ModelFile::from_rule(GridRule::life())
        .unwrap_or_else(|error| panic!("{error}"))
        .to_toml();
    for (after, key) in [
        ("description = ", "colour = 3\n"),
        ("[grid]\n", "depth = 3\n"),
        ("[[state]]\n", "shade = 1\n"),
        ("[[param]]\n", "apply = \"reload\"\n"),
        ("[step]\n", "file = \"life.wgsl\"\n"),
    ] {
        let at = text.find(after).unwrap_or_default();
        let line_start = if after.ends_with('\n') { at + after.len() } else { at };
        let edited = format!("{}{key}{}", &text[..line_start], &text[line_start..]);
        let expected_line = edited[..line_start].matches('\n').count() as u32 + 1;
        let error = ModelFile::parse(&edited)
            .err()
            .unwrap_or_else(|| panic!("`{key}` was accepted"));
        let ModelFileError::Toml(diagnostic) = &error else {
            panic!("`{key}` gave {error:?}");
        };
        let name = key.split(' ').next().unwrap_or_default();
        assert!(diagnostic.message.contains(name), "{diagnostic}");
        assert_eq!(diagnostic.line, Some(expected_line), "{diagnostic}");
    }

    let other = text.replace("format_version = 1", "format_version = 2");
    let error = ModelFile::parse(&other)
        .err()
        .unwrap_or_else(|| panic!("version 2 was accepted"));
    assert!(
        matches!(&error, ModelFileError::Format(diagnostic) if diagnostic.line == Some(2)),
        "{error:?}"
    );
    let other = text.replace("kind = \"gpu_grid\"", "kind = \"gpu_agents\"");
    assert!(matches!(ModelFile::parse(&other), Err(ModelFileError::Format(_))));

    // A later version with keys this build does not know reads as a later version, not as unknown keys.
    let later = format!(
        "{}[future]\nkey = 1\n",
        text.replace("format_version = 1", "format_version = 2")
    );
    let error = ModelFile::parse(&later)
        .err()
        .unwrap_or_else(|| panic!("version 2 was accepted"));
    assert!(
        matches!(&error, ModelFileError::Format(diagnostic) if diagnostic.line == Some(2)
            && diagnostic.message.contains("version 2")),
        "{error:?}"
    );
}

/// A problem in a declaration, which the rule's checks find, is placed at the declaration's table or key in the file.
#[test]
fn a_declaration_error_points_at_its_table() {
    let mut rule = GridRule::life();
    rule.states.push(RuleState::new("dead!", [0, 0, 0, 255]));
    rule.params.push(u32_param("radius", "Radius", 2, 1, 5));
    let text = ModelFile::from_rule(rule)
        .unwrap_or_else(|error| panic!("{error}"))
        .to_toml()
        .replace("id = \"radius\"", "id = \"seed\"")
        .replace("id = \"life_rule\"", "id = \"Life\"")
        .replace("width = 256", "width = 0");
    let error = ModelFile::parse(&text)
        .unwrap_or_else(|error| panic!("{error}"))
        .entry()
        .err()
        .unwrap_or_else(|| panic!("the declarations passed"));
    let line_at = |offset: usize| text[..offset].matches('\n').count() as u32 + 1;
    let line_of = |needle: &str| line_at(text.find(needle).unwrap_or_default());
    let third_state = text.match_indices("[[state]]").nth(2).map_or(0, |(at, _)| at);
    let second_param = text.match_indices("[[param]]").nth(1).map_or(0, |(at, _)| at);
    let mut placed: Vec<(u32, &str)> = error
        .diagnostics()
        .iter()
        .map(|diagnostic| (diagnostic.line.unwrap_or_default(), diagnostic.message.as_str()))
        .collect();
    placed.sort_unstable();
    let expected = [
        (line_of("id = \"Life\""), "rule id 'Life'"),
        (line_of("[grid]"), "the grid width is 0"),
        (line_at(third_state), "both give the constant `DEAD`"),
        (line_at(second_param), "parameter id 'seed' is reserved"),
    ];
    assert_eq!(placed.len(), expected.len(), "{}", lines_of(&error).join("\n"));
    for ((line, message), (expected_line, text_part)) in placed.iter().zip(expected) {
        assert_eq!(*line, expected_line, "{message}");
        assert!(message.contains(text_part), "{message}");
    }
}

/// The code of a file depends on what the file declares and on its step, and on nothing else: comments outside the
/// step, key order, blank lines, a colour without its alpha and the form of a number all leave it alone.
#[test]
fn the_canonical_code_ignores_formatting() {
    let canonical = ModelFile::from_rule(GridRule::life()).unwrap_or_else(|error| panic!("{error}"));
    let written = format!(
        "# A hand-written Life rule.\r\nkind = \"gpu_grid\"\r\nformat_version = 1\r\nformat = \"henad-model\"\r\n\
         name = 'Life rule'\r\nid = \"life_rule\"\r\ndescription = \"Conway's Game of Life, written as a grid \
         rule\"\r\n\r\n\r\n[[param]]\r\nstep = 1e-2\r\ndefault = 0.30\r\nmax = 1\r\nmin = 0\r\nkind = \"f32\"\r\n\
         label = \"Initial Density\"\r\nid = \"density\"\r\n\r\n[grid]\r\nheight = 256\r\nwidth = 256\r\n\r\n\
         [step]\r\nwgsl = '''\n{LIFE_STEP}'''\r\n\r\n[[state]]\r\nlabel = \"Dead\" # no share\r\n\
         color = [21, 21, 21]\r\n\r\n[[state]]\r\ninitial = \"density\"\r\nlabel = \"Alive\"\r\n\
         color = [0, 230, 118, 255]\r\n"
    );
    let read = ModelFile::parse(&written).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(read.to_toml(), canonical.to_toml());
    assert_eq!(read.code().hash(), canonical.code().hash());

    // A comment inside the step is the rule's text, and changes the code.
    let mut rule = GridRule::life();
    rule.step.push_str("// note\n");
    let commented = ModelFile::from_rule(rule).unwrap_or_else(|error| panic!("{error}"));
    assert_ne!(commented.code().hash(), canonical.code().hash());
    // So does a declaration.
    let mut rule = GridRule::life();
    rule.width = 255;
    let narrower = ModelFile::from_rule(rule).unwrap_or_else(|error| panic!("{error}"));
    assert_ne!(narrower.code().hash(), canonical.code().hash());
}

/// A problem in the step is reported at its line and character column in the file, wherever the string starts.
#[test]
fn a_wgsl_error_points_at_its_file_line() {
    let step = "fn step_cell(cell: u32, rng: ptr<function, u32>) -> u32 {\n    let é = 1u; return cell +;\n}\n";
    let file = ModelFile::from_rule(life_with(step))
        .unwrap_or_else(|error| panic!("{error}"))
        .to_toml();
    let error = ModelFile::parse(&file)
        .unwrap_or_else(|error| panic!("{error}"))
        .entry()
        .err()
        .unwrap_or_else(|| panic!("the step compiled"));
    let semicolon = file.find("cell +;").unwrap_or_default() + "cell +".len();
    let line = file[..semicolon].matches('\n').count() as u32 + 1;
    let ModelFileError::Rule(problems) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(problems[0].line, Some(line), "{}", lines_of(&error).join("\n"));
    assert_eq!(
        problems[0].column,
        Some("    let é = 1u; return cell +".chars().count() as u32 + 1)
    );
    assert_eq!(problems[0].span.as_ref().map(|span| span.start), Some(semicolon));

    // The step can start on the line of its opening `'''`. Its first line's columns then follow `wgsl = '''`.
    let inline = file.replace("wgsl = '''\n", "wgsl = '''");
    let error = ModelFile::parse(&inline)
        .unwrap_or_else(|error| panic!("{error}"))
        .entry()
        .err()
        .unwrap_or_else(|| panic!("the step compiled"));
    let semicolon = inline.find("cell +;").unwrap_or_default() + "cell +".len();
    assert_eq!(
        error.diagnostics()[0].span.as_ref().map(|span| span.start),
        Some(semicolon)
    );
    assert_eq!(error.diagnostics()[0].line, Some(line - 1));

    // A file with CRLF line ends gets spans into its text as written, one byte further per line before.
    let crlf = file.replace('\n', "\r\n");
    let error = ModelFile::parse(&crlf)
        .unwrap_or_else(|error| panic!("{error}"))
        .entry()
        .err()
        .unwrap_or_else(|| panic!("the step compiled"));
    let semicolon = crlf.find("cell +;").unwrap_or_default() + "cell +".len();
    let diagnostic = &error.diagnostics()[0];
    assert_eq!(diagnostic.span.as_ref().map(|span| span.start), Some(semicolon));
    assert_eq!(diagnostic.line, Some(line));
    assert_eq!(&crlf[semicolon..semicolon + 1], ";");
}

/// Every model file under `specs/models` reads and builds an entry of runtime code, and `life_rule.toml` holds the
/// rule `GridRule::life` returns.
#[test]
fn every_example_model_file_loads() {
    let models = Path::new(env!("CARGO_MANIFEST_DIR")).join("specs").join("models");
    let mut ids = Vec::new();
    for file in std::fs::read_dir(&models).unwrap_or_else(|error| panic!("{}: {error}", models.display())) {
        let path = file.unwrap_or_else(|error| panic!("{error}")).path();
        if path.extension().is_none_or(|extension| extension != "toml") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let file = ModelFile::parse(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let entry = file
            .entry()
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(entry.source().origin(), CodeOrigin::Runtime);
        assert_eq!(entry.source().code_hash(), Some(file.code().hash()));
        if path.file_name().is_some_and(|name| name == "life_rule.toml") {
            assert_eq!(
                file.rule().step,
                LIFE_STEP,
                "the Life file's step is the Life starter's"
            );
            let starter = ModelFile::from_rule(GridRule::life()).unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(file.to_toml(), starter.to_toml());
        }
        ids.push(entry.id().to_owned());
    }
    assert!(ids.iter().any(|id| id == "life_rule"), "{ids:?}");
}

/// Returns the Life starter with `step` as its text.
fn life_with(step: &str) -> GridRule {
    let mut rule = GridRule::life();
    rule.step = step.to_owned();
    rule
}
