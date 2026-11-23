//! The "?" help of the sheet editor: a mark beside a heading whose tooltip explains the heading in plain words.
//!
//! The mark is a label. Its tooltip shows under the pointer and holds the help's text and "Example:" with one example.
//! The mark takes no click and no keyboard focus. Its words come from [`henad_graph::sheet::sentence`]: the help of
//! each [`SheetSection`] and the Lines help beside the card, each with its own words for a world of agents where the
//! words for cells do not fit.

use egui::{AtomLayout, Frame, Label, RichText, Sense, Ui, WidgetInfo, WidgetType, vec2};
use henad_graph::sheet::sentence::{
    HELP_EXAMPLE_LABEL, HELP_LINES_NAME, LINES_HELP, LINES_HELP_AGENTS, LINES_HELP_AGENTS_EXAMPLE, LINES_HELP_EXAMPLE,
    SheetSection, WORLD_HELP_AGENTS,
};

use crate::icons::material_design_icons::MDI_HELP_CIRCLE_OUTLINE;

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

    /// Returns the accessible name of the help's mark, such as "Help: World" or "Help: lines".
    pub fn name(self) -> String {
        match self {
            Self::Section(section) => section.help_name(),
            Self::AgentWorld => SheetSection::World.help_name(),
            Self::Lines | Self::AgentLines => HELP_LINES_NAME.to_owned(),
        }
    }
}

/// Draws the "?" of `help` in the weak colour, a label that senses the pointer alone, whose tooltip shows the text and
/// "Example:" with the example in the weak colour, wrapping at [`HELP_WIDTH`].
///
/// The icon sits inside the theme's horizontal button padding. The mark's accessible node is a label named by
/// [`Help::name`], with the same words as its description.
pub fn help_mark(ui: &mut Ui, help: Help) {
    let mark = AtomLayout::new(RichText::new(MDI_HELP_CIRCLE_OUTLINE).weak())
        .frame(Frame::new().inner_margin(vec2(ui.spacing().button_padding.x, 0.0)))
        .sense(Sense::hover())
        .show(ui)
        .response;
    mark.widget_info(|| WidgetInfo::new(WidgetType::Label));
    let description = format!("{} {HELP_EXAMPLE_LABEL} {}", help.text(), help.example());
    ui.ctx().accesskit_node_builder(mark.id, |node| {
        node.set_label(help.name());
        node.set_description(description);
    });
    mark.on_hover_ui(|ui| {
        ui.style_mut().interaction.selectable_labels = false;
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
