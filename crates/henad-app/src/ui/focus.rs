//! Keyboard focus order of the app: the dividers of resizable panels, which Tab passes over, and Tab from the last
//! widget of a frame to the first.
//!
//! egui puts every widget that senses a click or a drag in the order of Tab, a panel's divider included. A divider has
//! no accessible name and cannot be moved from the keyboard. Left to egui, Tab from the last widget also leaves no
//! widget focused in its frame, and the first widget takes the focus only in the next frame.

use egui::{Context, FocusDirection, Id, Key, Ui};

/// Returns the id of the divider of the panel `panel`, the handle a drag resizes the panel by.
///
/// egui derives it from the panel's id.
pub fn panel_divider(panel: Id) -> Id {
    panel.with("__resize")
}

/// Runs `show`, which draws the widget `id`, with the widget out of the keyboard focus order: Tab and Shift+Tab pass
/// over it.
pub fn without_focus_stop<R>(ctx: &Context, id: Id, show: impl FnOnce() -> R) -> R {
    // Shift+Tab focuses the widget before the one it leaves in the frame after its press. In a frame with no Tab, the
    // widget passes the focus on to the widget before it, which takes it in the next frame.
    if ctx.memory(|memory| memory.has_focus(id)) && !ctx.input(|input| input.key_pressed(Key::Tab)) {
        ctx.memory_mut(|memory| memory.move_focus(FocusDirection::Previous));
        ctx.request_repaint();
    }
    let shown = show();
    // Tab focuses the widget after the one it leaves within the frame. The next widget drawn takes the focus instead.
    if ctx.memory(|memory| memory.has_focus(id)) && tab_pressed(ctx) {
        ctx.memory_mut(|memory| {
            memory.surrender_focus(id);
            memory.move_focus(FocusDirection::Next);
        });
    }
    shown
}

/// Installs the plugin of `ctx` that makes Tab from the last widget of a frame focus the first.
pub fn install(ctx: &Context) {
    ctx.add_plugin(TabWraps::default());
}

/// Returns whether Tab with no modifier was pressed in the pass of `ctx`.
fn tab_pressed(ctx: &Context) -> bool {
    ctx.input(|input| input.key_pressed(Key::Tab) && input.modifiers.is_none())
}

/// Plugin that makes Tab from the last widget of a frame focus the first widget within the frame.
///
/// After Tab leaves the last widget, egui focuses the first widget only in a later pass. The plugin asks egui for a
/// second pass of the frame, in which the first widget takes the focus.
#[derive(Debug, Default)]
struct TabWraps {
    /// Widget that held the focus as the pass began.
    focused_at_start: Option<Id>,
    /// Whether the next pass moves the focus to its first widget.
    wraps: bool,
}

impl egui::Plugin for TabWraps {
    fn debug_name(&self) -> &'static str {
        "henad_tab_wraps"
    }

    fn on_begin_pass(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx();
        if std::mem::take(&mut self.wraps) {
            ctx.memory_mut(|memory| memory.move_focus(FocusDirection::Next));
        }
        self.focused_at_start = ctx.memory(|memory| memory.focused());
    }

    fn on_end_pass(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx();
        if self.focused_at_start.is_some() && tab_pressed(ctx) && ctx.memory(|memory| memory.focused()).is_none() {
            self.wraps = true;
            ctx.request_discard("Tab from the last widget focuses the first");
        }
    }
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Id, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};

    use super::{install, panel_divider, without_focus_stop};

    /// Returns the press and the release of Tab, with Shift when `shift` is set.
    fn tab(shift: bool) -> Vec<Event> {
        let modifiers = if shift { Modifiers::SHIFT } else { Modifiers::NONE };
        [true, false]
            .into_iter()
            .map(|pressed| Event::Key {
                key: Key::Tab,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            })
            .collect()
    }

    /// Returns the press and the release of the primary button at `position`.
    fn click(position: Pos2) -> Vec<Event> {
        [true, false]
            .into_iter()
            .map(|pressed| Event::PointerButton {
                pos: position,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            })
            .collect()
    }

    /// Widgets of a frame of [`Row`] and what they reported.
    #[derive(Debug)]
    struct Drawn {
        /// Ids of the buttons A, B and C, in order.
        buttons: Vec<Id>,
        /// Rectangle of button B.
        second: Rect,
        /// Number of clicks button B reported, summed over the passes of the frame.
        clicks: u32,
        /// Rectangle of the left panel.
        panel: Rect,
    }

    /// A row of three buttons, A, B and C, with a resizable left panel, whose divider [`without_focus_stop`] takes
    /// out of the order of Tab, drawn after A.
    struct Row {
        ctx: Context,
        drawn: Drawn,
    }

    impl Row {
        fn new() -> Self {
            let ctx = Context::default();
            install(&ctx);
            Self {
                ctx,
                drawn: Drawn {
                    buttons: Vec::new(),
                    second: Rect::NOTHING,
                    clicks: 0,
                    panel: Rect::NOTHING,
                },
            }
        }

        /// Runs one frame with `events`, and returns the number of passes it took.
        fn frame(&mut self, events: Vec<Event>) -> u32 {
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
                events,
                ..RawInput::default()
            };
            let drawn = &mut self.drawn;
            let ctx = self.ctx.clone();
            let output = self.ctx.run_ui(input, |ui| {
                drawn.buttons.clear();
                drawn.buttons.push(ui.button("A").id);
                let panel = Id::new("panel");
                let shown = without_focus_stop(&ctx, panel_divider(panel), || {
                    egui::Panel::left(panel)
                        .resizable(true)
                        .default_size(200.0)
                        .show(ui, |ui| {
                            ui.label("Panel");
                            ui.take_available_space();
                        })
                });
                drawn.panel = shown.response.rect;
                let second = ui.button("B");
                drawn.second = second.rect;
                drawn.clicks += u32::from(second.clicked());
                drawn.buttons.push(second.id);
                drawn.buttons.push(ui.button("C").id);
            });
            let passes = output.platform_output.num_completed_passes as u32;
            output.drop_without_applying_deltas();
            passes
        }

        /// Returns the index of the focused button, `None` while no button holds the focus.
        fn focused(&self) -> Option<usize> {
            let focused = self.ctx.memory(|memory| memory.focused())?;
            self.drawn.buttons.iter().position(|id| *id == focused)
        }
    }

    #[test]
    fn tab_passes_over_a_divider_and_wraps_to_the_first_widget_in_the_same_frame() {
        let mut row = Row::new();
        row.frame(Vec::new());
        assert_eq!(row.frame(tab(false)), 1);
        assert_eq!(row.focused(), Some(0));
        assert_eq!(row.frame(tab(false)), 1, "a Tab that does not wrap takes one pass");
        assert_eq!(row.focused(), Some(1), "Tab from A passes over the divider");
        row.frame(tab(false));
        assert_eq!(row.focused(), Some(2));
        assert_eq!(row.frame(tab(false)), 2, "a Tab that wraps takes a second pass");
        assert_eq!(row.focused(), Some(0), "Tab from the last widget focuses the first");
    }

    #[test]
    fn shift_tab_passes_over_a_divider_and_wraps_to_the_last_widget() {
        let mut row = Row::new();
        row.frame(Vec::new());
        row.frame(tab(false));
        row.frame(tab(false));
        assert_eq!(row.focused(), Some(1));
        // egui moves the focus of Shift+Tab a frame after its press, and the divider passes it on a frame later.
        assert_eq!(row.frame(tab(true)), 1);
        row.frame(Vec::new());
        row.frame(Vec::new());
        assert_eq!(row.focused(), Some(0), "Shift+Tab from B passes over the divider");
        assert_eq!(
            row.frame(tab(true)),
            1,
            "Shift+Tab from the first widget takes one pass"
        );
        row.frame(Vec::new());
        assert_eq!(
            row.focused(),
            Some(2),
            "Shift+Tab from the first widget focuses the last"
        );
    }

    #[test]
    fn the_second_pass_handles_no_input_again() {
        let mut row = Row::new();
        row.frame(Vec::new());
        for _ in 0..3 {
            row.frame(tab(false));
        }
        assert_eq!(row.focused(), Some(2));
        let mut events = click(row.drawn.second.center());
        events.extend(tab(false));
        assert_eq!(row.frame(events), 2);
        assert_eq!(row.drawn.clicks, 1, "the click counts once over the two passes");
        assert_eq!(row.focused(), Some(0));
    }

    #[test]
    fn a_divider_out_of_the_order_still_resizes_its_panel() {
        let mut row = Row::new();
        row.frame(Vec::new());
        let width = row.drawn.panel.width();
        let edge = row.drawn.panel.right_center();
        let to = edge + egui::vec2(60.0, 0.0);
        row.frame(vec![Event::PointerMoved(edge)]);
        row.frame(vec![Event::PointerButton {
            pos: edge,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        for step in 1..=6 {
            row.frame(vec![Event::PointerMoved(edge + egui::vec2(10.0 * step as f32, 0.0))]);
        }
        row.frame(vec![Event::PointerButton {
            pos: to,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
        row.frame(Vec::new());
        assert!(
            (row.drawn.panel.width() - (width + 60.0)).abs() < 2.0,
            "a drag of 60 points widens the panel from {width} to {}",
            row.drawn.panel.width()
        );
        assert_eq!(
            row.ctx.memory(|memory| memory.focused()),
            None,
            "the drag focuses nothing"
        );
    }
}
