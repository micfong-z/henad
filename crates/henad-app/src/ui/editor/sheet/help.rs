//! The "?" help of the sheet editor: a small button beside a heading whose tooltip explains the heading in plain words.
//!
//! The tooltip shows under the pointer and while the keyboard holds the button's focus, and holds the help's text and
//! "Example:" with one example. A click opens nothing. Its words come from [`henad_graph::sheet::sentence`]: the help
//! of each [`SheetSection`] and the Lines help beside the card, each with its own words for a world of agents where the
//! words for cells do not fit.

use egui::{Button, Label, RichText, Ui};
use henad_graph::sheet::sentence::{
    HELP_EXAMPLE_LABEL, HELP_LINES_NAME, LINES_HELP, LINES_HELP_AGENTS, LINES_HELP_AGENTS_EXAMPLE, LINES_HELP_EXAMPLE,
    SheetSection, WORLD_HELP_AGENTS,
};

use super::widgets::FocusTooltip as _;
use crate::icons::material_design_icons::MDI_HELP_CIRCLE_OUTLINE;
use crate::ui::sweep::layout::add_button;

/// Width in points at which the words of a help tooltip wrap.
const HELP_WIDTH: f32 = 320.0;

/// One help of the sheet editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Help {
    /// The help of a section of the Sheet pane.
    Section(SheetSection),
    /// The help of the card's lines.
    Lines,
    /// The help of the World section of a world of agents.
    AgentWorld,
    /// The help of the lines of a kind of agent's card.
    AgentLines,
}

impl Help {
    /// Text of the help.
    pub fn text(self) -> &'static str {
        match self {
            Self::Section(section) => section.help(),
            Self::AgentWorld => WORLD_HELP_AGENTS,
            Self::Lines => LINES_HELP,
            Self::AgentLines => LINES_HELP_AGENTS,
        }
    }

    /// The one example the help gives after "Example:".
    pub fn example(self) -> &'static str {
        match self {
            Self::Section(section) => section.help_example(),
            Self::AgentWorld => SheetSection::World.help_example(),
            Self::Lines => LINES_HELP_EXAMPLE,
            Self::AgentLines => LINES_HELP_AGENTS_EXAMPLE,
        }
    }

    /// Returns the accessible name of the help button, such as "Help: World" or "Help: lines".
    pub fn name(self) -> String {
        match self {
            Self::Section(section) => section.help_name(),
            Self::AgentWorld => SheetSection::World.help_name(),
            Self::Lines | Self::AgentLines => HELP_LINES_NAME.to_owned(),
        }
    }
}

/// Draws the "?" of `help`, a frameless button with egui's button frame under the pointer and on focus, whose tooltip,
/// under the pointer or while the keyboard holds its focus, shows the text and "Example:" with the example in the weak
/// colour, wrapping at [`HELP_WIDTH`].
///
/// The button's accessible description holds the same words.
pub fn help_button(ui: &mut Ui, help: Help) {
    let button = add_button(
        ui,
        true,
        Button::new(RichText::new(MDI_HELP_CIRCLE_OUTLINE).weak())
            .frame_when_inactive(false)
            .small(),
        &help.name(),
    );
    let description = format!("{} {HELP_EXAMPLE_LABEL} {}", help.text(), help.example());
    ui.ctx()
        .accesskit_node_builder(button.id, |node| node.set_description(description));
    button.on_hover_or_focus_ui(|ui| {
        ui.set_max_width(HELP_WIDTH);
        ui.add(Label::new(help.text()).wrap());
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.weak(HELP_EXAMPLE_LABEL);
            ui.weak(help.example());
        });
    });
}
