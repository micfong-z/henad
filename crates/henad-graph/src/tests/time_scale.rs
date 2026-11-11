//! The time scale of a graph file: the `hidden` and `tick_unit` keys of a `[[param]]` table, their checker rules, the
//! edits that keep them, and the entry that carries the time scale.

use henad_compute::entry::ModelEntry;
use henad_core::metadata::{TimeScale, TimeUnit};

use super::support::{graph_errors, graph_refusal};
use crate::GraphFile;
use crate::edit::{EditCommand, GraphBuilder};
use crate::starters::starter;
use crate::wgsl::{AgentProgram, GridProgram};

/// A tick length in days, as a sheet with rates lowers it.
const TICK_LENGTH: &str = r#"[[param]]
id = "tick_length"
label = "One tick is (days)"
kind = "f32"
min = 0.01
max = 30.0
default = 1.0
hidden = true
tick_unit = "day"
"#;

/// Returns the starter `file_name` with `params` among its parameters.
fn starter_with(file_name: &str, params: &str) -> String {
    let text = starter(file_name).expect("the starter").text;
    let first_pass = text.find("[[pass]]").expect("the starter has a pass");
    format!("{}{params}\n{}", &text[..first_pass], &text[first_pass..])
}

/// Returns the Life starter with `params` among its parameters.
fn life_with(params: &str) -> String {
    starter_with("game_of_life.toml", params)
}

#[test]
fn the_two_keys_round_trip_through_the_canonical_text() {
    let file = GraphFile::parse(&life_with(TICK_LENGTH)).expect("the file reads");
    let document = file.document();
    assert_eq!(document.time, Some(TimeScale::new(TimeUnit::Day, "tick_length")));
    let param = document
        .params
        .iter()
        .find(|param| param.id() == "tick_length")
        .expect("the tick length");
    assert!(param.is_hidden());
    assert_eq!(document.params.iter().filter(|param| param.is_hidden()).count(), 1);

    let text = file.to_toml();
    assert!(
        text.contains("default = 1.0\nhidden = true\ntick_unit = \"day\"\n"),
        "{text}"
    );
    let again = GraphFile::parse(&text).expect("the canonical text reads");
    assert_eq!(again.to_toml(), text);
    assert_eq!(again.code(), file.code());
    assert!(graph_errors(&text).is_empty(), "{:?}", graph_errors(&text));
}

#[test]
fn a_hidden_parameter_needs_no_unit_and_a_shown_one_writes_no_key() {
    let hidden = TICK_LENGTH.replace("tick_unit = \"day\"\n", "");
    let file = GraphFile::parse(&life_with(&hidden)).expect("the file reads");
    assert_eq!(file.document().time, None);
    assert!(file.to_toml().contains("hidden = true\n"));
    assert!(!file.to_toml().contains("tick_unit"));

    let shown = hidden.replace("hidden = true\n", "hidden = false\n");
    let file = GraphFile::parse(&life_with(&shown)).expect("the file reads");
    assert!(
        !file.to_toml().contains("hidden"),
        "`hidden = false` is the default and is left out"
    );
}

#[test]
fn the_reader_refuses_a_second_tick_length_and_an_unknown_unit() {
    let second = TICK_LENGTH.replace("\"tick_length\"", "\"other_length\"");
    let message = graph_refusal(&life_with(&format!("{TICK_LENGTH}\n{second}"))).message;
    assert!(
        message.contains("'tick_length' and 'other_length' both set `tick_unit`"),
        "{message}"
    );

    let message = graph_refusal(&life_with(&TICK_LENGTH.replace("\"day\"", "\"fortnight\""))).message;
    assert!(message.contains("'fortnight'"), "{message}");
    assert!(
        message.contains("second, minute, hour, day, week, month or year"),
        "{message}"
    );
    let message = graph_refusal(&life_with(&TICK_LENGTH.replace("\"day\"", "\"days\""))).message;
    assert!(message.contains("'days'"), "a plural is not a unit's name: {message}");
}

#[test]
fn the_checker_holds_a_tick_length_to_a_hidden_positive_f32() {
    let unhidden = TICK_LENGTH.replace("hidden = true\n", "");
    assert_eq!(
        graph_errors(&life_with(&unhidden)),
        ["the parameter 'tick_length' sets `tick_unit` and needs `hidden = true`"]
    );

    let at_zero = TICK_LENGTH.replace("min = 0.01", "min = 0.0");
    assert_eq!(
        graph_errors(&life_with(&at_zero)),
        ["the parameter 'tick_length' sets `tick_unit` and needs a `min` above 0"]
    );
    let below_zero = TICK_LENGTH.replace("min = 0.01", "min = -1.0");
    assert_eq!(graph_errors(&life_with(&below_zero)).len(), 1);

    let whole = "[[param]]\nid = \"tick_length\"\nlabel = \"One tick is (days)\"\nkind = \"u32\"\nmin = 1\nmax = 30\n\
                 default = 1\nhidden = true\ntick_unit = \"day\"\n";
    assert_eq!(
        graph_errors(&life_with(whole)),
        ["the parameter 'tick_length' sets `tick_unit` and needs `kind = \"f32\"`"]
    );
}

#[test]
fn renaming_or_removing_the_tick_length_keeps_the_time_scale_in_step() {
    let file = GraphFile::parse(&life_with(TICK_LENGTH)).expect("the file reads");
    let mut builder = GraphBuilder::from_file(&file);
    builder
        .apply(EditCommand::RenameParam {
            from: "tick_length".to_owned(),
            to: "step_length".to_owned(),
        })
        .expect("the parameter renames");
    let renamed = builder.to_file().expect("the graph reads back");
    assert_eq!(
        renamed.document().time,
        Some(TimeScale::new(TimeUnit::Day, "step_length"))
    );
    assert!(renamed.check().errors().next().is_none(), "the rename keeps `hidden`");

    builder
        .apply(EditCommand::RemoveParam("step_length".to_owned()))
        .expect("the parameter removes");
    let removed = builder.to_file().expect("the graph reads back");
    assert_eq!(removed.document().time, None);
    assert!(!builder.to_toml().contains("tick_unit"));
}

/// Checks that `timed`, the entry of a graph with [`TICK_LENGTH`], carries its time scale with the tick length hidden,
/// and that `untimed`, the entry of the same graph without it, carries none.
fn assert_time_scale(timed: &ModelEntry, untimed: &ModelEntry) {
    let time = timed.metadata().time.clone().expect("a time scale");
    assert_eq!(time, TimeScale::new(TimeUnit::Day, "tick_length"));
    let index = time
        .tick_length_index(timed.param_descriptors())
        .expect("the entry declares the tick length");
    assert!(timed.param_descriptors()[index].is_hidden());
    assert_eq!(untimed.metadata().time, None);
}

#[test]
fn the_entry_carries_the_time_scale_and_a_graph_without_one_carries_none() {
    let entry = |text: &str| {
        let file = GraphFile::parse(text).expect("the file reads");
        GridProgram::compile(&file)
            .unwrap_or_else(|problems| panic!("{problems:#?}"))
            .entry()
    };
    assert_time_scale(&entry(&life_with(TICK_LENGTH)), &entry(&life_with("")));
}

#[test]
fn an_agents_entry_carries_the_time_scale_and_one_without_carries_none() {
    let entry = |text: &str| {
        let file = GraphFile::parse(text).expect("the file reads");
        AgentProgram::compile(&file)
            .unwrap_or_else(|problems| panic!("{problems:#?}"))
            .entry()
    };
    assert_time_scale(
        &entry(&starter_with("random_walk.toml", TICK_LENGTH)),
        &entry(&starter_with("random_walk.toml", "")),
    );
}
