//! The "?" help of the sheet editor: a small button beside a heading that opens a popover in plain words.
//!
//! Each popover holds its text, "Example:" with one example, and Close help. Its words come from
//! [`henad_graph::sheet::sentence`]: the help of each [`SheetSection`] and the Lines help beside the card, each with
//! its own words for a world of agents where the words for cells do not fit.

use egui::{Button, Popup, PopupCloseBehavior, RichText, Ui};
use henad_graph::sheet::sentence::{
    CLOSE_HELP, HELP_EXAMPLE_LABEL, HELP_LINES_NAME, LINES_HELP, LINES_HELP_AGENTS, LINES_HELP_AGENTS_EXAMPLE,
    LINES_HELP_EXAMPLE, LINES_HELP_HEADING, SheetSection, WORLD_HELP_AGENTS, help_dialog_name,
};

use super::accessible_group;
use crate::icons::material_design_icons::MDI_HELP_CIRCLE_OUTLINE;
use crate::ui::sweep::layout::add_button;

/// Width in points of a help popover.
const HELP_WIDTH: f32 = 320.0;

/// One help popover of the sheet editor.
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
    /// Heading of the popover, the heading its button stands beside.
    pub fn heading(self) -> &'static str {
        match self {
            Self::Section(section) => section.heading(),
            Self::AgentWorld => SheetSection::World.heading(),
            Self::Lines | Self::AgentLines => LINES_HELP_HEADING,
        }
    }

    /// Text of the popover.
    pub fn text(self) -> &'static str {
        match self {
            Self::Section(section) => section.help(),
            Self::AgentWorld => WORLD_HELP_AGENTS,
            Self::Lines => LINES_HELP,
            Self::AgentLines => LINES_HELP_AGENTS,
        }
    }

    /// The one example the popover gives after "Example:".
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

/// Draws the "?" of `help` and, while it is open, its popover below it.
pub fn help_button(ui: &mut Ui, help: Help) {
    let name = help.name();
    let heading = help.heading();
    let button = add_button(
        ui,
        true,
        Button::new(RichText::new(MDI_HELP_CIRCLE_OUTLINE).weak())
            .frame(false)
            .small(),
        &name,
    );
    // The popover's id derives from the button's, so each button owns its popover.
    let popup = Popup::from_toggle_button_response(&button)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .width(HELP_WIDTH)
        .show(|ui| {
            ui.strong(heading);
            ui.label(help.text());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(HELP_EXAMPLE_LABEL).strong());
                ui.label(help.example());
            });
            if ui.button(CLOSE_HELP).clicked() {
                ui.close();
            }
            ui.unique_id()
        });
    if let Some(popup) = popup {
        // Named apart from the button, so a locator of the button's name finds the button alone.
        accessible_group(
            ui.ctx(),
            popup.inner,
            egui::accesskit::Role::Dialog,
            &help_dialog_name(heading),
            popup.response.rect,
        );
    }
}
