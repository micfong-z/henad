//! Words of the conversion of a sheet to a node graph.

use super::grammar::{number_list, word_list};

// Conversion to a node graph. These words sit under the exemption for the controls that open a node graph, and
// `FORBIDDEN_WORDS` does not cover them.

/// Tooltip of the disabled Convert while the card holds an error.
pub const CONVERT_NEEDS_FIXES: &str = "Fix the problems on the card to convert.";

/// Tooltip of the disabled Convert while the card holds a line that does not run.
pub const CONVERT_NEEDS_FINISHED_LINES: &str = "Some lines do not run yet. Finish or delete them to convert.";

/// Heading of the conversion dialog.
pub const CONVERT_HEADING: &str = "Convert to node graph";

/// Returns the name of the copy a conversion makes of the model `name`, such as "Game of Life (expert)".
pub fn expert_copy_name(name: &str) -> String {
    format!("{name} (expert)")
}

/// Returns the two paragraphs of the conversion dialog for the model `name`, each sentence on a line of its own.
pub fn convert_text(name: &str) -> [String; 2] {
    [
        format!(
            "{name} will be copied to a new model, {}, as a node graph.\n{name} will not be changed.",
            expert_copy_name(name)
        ),
        "Every node Henad runs can be edited in the node graph.\nThe copy has no sheet or cards, so lines, fire counts \
         and \"otherwise\" will not appear in it.\nChanges to the copy cannot be converted back to lines."
            .to_owned(),
    ]
}

/// Primary action of the conversion dialog.
pub const CONVERT: &str = "Convert";

/// Returns the name of copy `number` of the model `name`, counted from 1: "Game of Life (expert)" for the first copy
/// and "Game of Life (expert 2)" for the second.
pub fn expert_copy_number_name(name: &str, number: u32) -> String {
    if number <= 1 {
        expert_copy_name(name)
    } else {
        format!("{name} (expert {number})")
    }
}

/// Returns the description of a copy converted from the model `name`.
pub fn converted_description(name: &str) -> String {
    format!("Converted from the author model {name}. Edit it as a node graph.")
}

/// Returns the words a copy shows when its source `name` no longer matches the record of the conversion.
pub fn original_changed(name: &str) -> String {
    format!("{name} has changed since this conversion.")
}

/// Button beside the Diagram that opens the conversion dialog.
pub const EDIT_AS_NODE_GRAPH: &str = "Edit as node graph";

/// Accessible name of the sheet toolbar's menu of the model's actions.
pub const MODEL_ACTIONS: &str = "Model actions";

/// Returns `date`, a date written as `2026-11-07`, as the banner of a copy writes it: "7 Nov 2026". A date that does
/// not read as one is returned as written.
pub fn converted_date(date: &str) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut fields = date.split('-');
    let parsed = (|| {
        let year: u32 = fields.next()?.parse().ok()?;
        let month: usize = fields.next()?.parse().ok()?;
        let day: u32 = fields.next()?.parse().ok()?;
        let month = MONTHS.get(month.checked_sub(1)?)?;
        (fields.next().is_none() && (1..=31).contains(&day)).then(|| format!("{day} {month} {year}"))
    })();
    parsed.unwrap_or_else(|| date.to_owned())
}

/// Returns the banner of a copy converted from the model `name` on `date` and not edited since, such as "Node graph
/// converted from Game of Life on 7 Nov 2026. Frames show the line each node came from."
pub fn converted_banner(name: &str, date: &str) -> String {
    format!(
        "Node graph converted from {name} on {}. Frames show the line each node came from.",
        converted_date(date)
    )
}

/// Returns the banner of a copy converted from the model `name` on `date` and edited since, where `edits` says what
/// the edits changed, as [`node_edits`] writes it.
pub fn edited_banner(name: &str, date: &str, edits: &str) -> String {
    format!(
        "Node graph converted from {name} on {}. Edited since: {edits}.",
        converted_date(date)
    )
}

/// Returns the nodes an edit of a copy added, removed and changed, such as "2 nodes added, 1 changed" or "1 node
/// changed", or "no node changed" when it changed none.
///
/// The first part names the noun, whichever part it is, and the parts after it leave the noun out.
pub fn node_edits(added: usize, removed: usize, changed: usize) -> String {
    let parts: Vec<(usize, &str)> = [(added, "added"), (removed, "removed"), (changed, "changed")]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .collect();
    if parts.is_empty() {
        return "no node changed".to_owned();
    }
    parts
        .iter()
        .enumerate()
        .map(|(index, (count, verb))| match (index, count) {
            (0, 1) => format!("1 node {verb}"),
            (0, _) => format!("{count} nodes {verb}"),
            _ => format!("{count} {verb}"),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Returns the banner of a copy whose source `name` has changed since the conversion.
pub fn original_changed_banner(name: &str) -> String {
    format!("{} Convert it again for a new node graph.", original_changed(name))
}

/// Returns the banner of a copy whose source `name` is not found beside its file.
pub fn original_not_beside(name: &str) -> String {
    format!("{name} is not beside this file. Frames still show the line each node came from.")
}

/// Returns the banner's button that opens the source `name` of a copy, such as "Open Game of Life".
pub fn open_original(name: &str) -> String {
    format!("Open {name}")
}

/// Returns the line under a sheet's name that opens its copy `copy`, such as "Converted to node graph: Game of Life
/// (expert)".
pub fn converted_link(copy: &str) -> String {
    format!("Converted to node graph: {copy}")
}

/// Heading of the side panel's section about the source of a copy.
pub const FROM_AUTHOR_MODEL: &str = "From the author model";

/// Banner's toggle that lists the nodes an edit of a copy changed and marks them on the canvas.
pub const SHOW_CHANGES: &str = "Show changes";

/// Heading of the side panel's list of the nodes an edit of a copy changed.
pub const CHANGES_HEADING: &str = "Changes since conversion";

/// Group of the changes list that holds the nodes in no frame. The expert added these nodes after conversion.
pub const IN_NO_FRAME: &str = "Not in a frame";

/// Badge of a node the expert added to a copy, and the list's word for it.
pub const NODE_ADDED: &str = "Added";

/// List's word for a node the expert removed from a copy.
pub const NODE_REMOVED: &str = "Removed";

/// Badge of a node the expert changed in a copy, and the list's word for it.
pub const NODE_CHANGED: &str = "Changed";

/// Banner's button that runs a copy beside its source and plots both.
pub const COMPARE_RUNS: &str = "Compare runs";

/// Returns the line shown while a copy `copy` and its source `name` run for `ticks` ticks at `seed`, such as "Running
/// Game of Life and Game of Life (expert) for 200 ticks at seed 1."
pub fn compare_running(name: &str, copy: &str, ticks: u64, seed: u64) -> String {
    format!("Running {name} and {copy} for {ticks} ticks at seed {seed}.")
}

/// Returns the line shown once a copy `copy` matched its source `name` at every tick up to `ticks`.
pub fn runs_match(name: &str, copy: &str, ticks: u64) -> String {
    format!("{copy} matches {name} at every tick from 0 to {ticks}.")
}

/// Returns the line shown once a copy `copy` first differed from its source `name`, at `tick` in the measure
/// `measure`, where the copy read `copy_value` and the source `value`.
pub fn runs_differ(name: &str, copy: &str, tick: u64, measure: &str, copy_value: &str, value: &str) -> String {
    format!("{copy} first differs from {name} at tick {tick}: {measure} is {copy_value} instead of {value}.")
}

/// Returns the line shown once the runs of a copy `copy` and its source `name` differ, when either model does not
/// replay exactly.
///
/// Two runs of such a model at one seed can differ, so no tick of the difference is named.
pub fn runs_do_not_replay(name: &str, copy: &str) -> String {
    format!(
        "{name} does not replay exactly. Two runs with the same seed can differ, so compare {copy} with {name} in the \
         plot."
    )
}

/// Returns the line shown when a copy and its source share no measure to compare.
pub fn runs_share_no_measure(name: &str, copy: &str) -> String {
    format!("{copy} and {name} have no measures in common.")
}

/// Returns the line shown when the runs of a comparison failed with `message`.
pub fn compare_failed(message: &str) -> String {
    format!("Comparison failed: {message}")
}

/// Returns the legend entry of the measure `measure` in the run of the model `name`, such as "Alive, Game of Life".
pub fn compare_series(measure: &str, name: &str) -> String {
    format!("{measure}, {name}")
}

/// Tooltip of Compare runs in a browser. A browser runs no sweep of a GPU model.
pub const COMPARE_NEEDS_NATIVE: &str = "Unavailable in a browser";

/// Button that closes the comparison of a copy and its source.
pub const CLOSE_COMPARISON: &str = "Close comparison";

/// Returns the title of the frame around the nodes of one line in a converted copy, such as "Line 2: Become Alive
/// from Dead, when Alive neighbours is 3", where `prefix` is the line prefix.
pub fn line_frame_title(prefix: &str, sentence: &str) -> String {
    format!("{prefix}: {sentence}")
}

/// Returns the prefix of several lines in a frame title: "Lines 2 and 3", or "Clear lines 1 and 2" on the card of
/// the button `button`.
pub fn lines_prefix(button: Option<&str>, numbers: &[usize]) -> String {
    match button {
        Some(button) => format!("{button} lines {}", number_list(numbers)),
        None => format!("Lines {}", number_list(numbers)),
    }
}

/// Returns the words of the nodes that work out the new value of `property` from several changes, such as "the new
/// Status (the first change that applies wins)".
pub fn new_value_words(property: &str) -> String {
    format!("the new {property} (the first change that applies wins)")
}

/// Returns the words of a neighbour's property read inside a look, such as "their Status".
pub fn their_property_words(property: &str) -> String {
    format!("their {property}")
}

/// Returns the title of a frame around the nodes of a part of the sheet in a converted copy, such as "Sheet, measure:
/// Alive", where `words` names the part.
pub fn sheet_frame_title(words: &str) -> String {
    format!("Sheet, {words}")
}

/// Returns the words of the start of `property`, such as "Status starts as".
pub fn starts_frame_words(property: &str) -> String {
    format!("{property} starts as")
}

/// Returns the words of the measures named `names`: "measure: Alive", or "measures: Alive and Dead".
pub fn measure_frame_words(names: &[String]) -> String {
    match names {
        [name] => format!("measure: {name}"),
        names => format!("measures: {}", word_list(names)),
    }
}

/// Returns the words of the button named `name`, such as "button Clear".
pub fn button_frame_words(name: &str) -> String {
    format!("button {name}")
}

/// Returns the words of the colour rule of each `thing` over `property`, such as "colour each cell by Status".
pub fn color_frame_words(thing: &str, property: &str) -> String {
    format!("colour each {thing} by {property}")
}

/// Words of a part of the sheet that no other words name, in the title of its frame in a converted copy.
pub const SHEET_PART_WORDS: &str = "shared nodes";
