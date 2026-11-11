//! The scheduled actions of the Parameters panel: the actions each build runs at their ticks.

use crate::icons::material_design_icons::{MDI_DELETE_OUTLINE, MDI_PLUS, MDI_RESTART};
use crate::state::AppState;
use crate::ui::sweep::layout::icon_button;
use henad_core::action::{ActionDescriptor, Schedule, Scheduled};

/// Draws the actions each build runs at their ticks, with a row to add an action.
pub(super) fn schedule_ui(ui: &mut egui::Ui, app: &mut AppState, actions: &[ActionDescriptor]) {
    let pending = app.schedule_pending();
    let mut heading = egui::RichText::new("Scheduled actions").strong();
    let hint = if pending {
        heading = heading.color(ui.visuals().warn_fg_color);
        format!("Schedule changed. Press {MDI_RESTART}\u{a0}Build to apply.")
    } else {
        "Actions at tick 0 run when the model is built. Later ones run after the step that reaches their tick."
            .to_owned()
    };

    ui.add_space(4.0);
    ui.label(heading).on_hover_text(hint);

    let mut removed = None;
    for (position, entry) in app.schedule.entries().iter().enumerate() {
        let label = actions
            .get(entry.index)
            .map_or(entry.id.as_str(), |action| action.label());
        let text = format!("{label} at tick {}", entry.tick);
        ui.horizontal(|ui| {
            if remove_button(ui, &text).clicked() {
                removed = Some(position);
            }
            ui.label(text);
        });
    }
    if let Some(position) = removed {
        app.schedule = without_entry(&app.schedule, position);
    }

    let mut added = false;
    ui.horizontal(|ui| {
        let selected = actions
            .get(app.schedule_action_input)
            .map_or("", |action| action.label());
        egui::ComboBox::from_id_salt("scheduled_action")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for (index, action) in actions.iter().enumerate() {
                    ui.selectable_value(&mut app.schedule_action_input, index, action.label());
                }
            });
        let tick_label = ui.label("at tick");
        ui.add(egui::DragValue::new(&mut app.schedule_tick_input))
            .labelled_by(tick_label.id);
        added = ui
            .button(format!("{MDI_PLUS} Add"))
            .on_hover_text("Schedule this action at this tick")
            .clicked();
    });
    if added && let Some(action) = actions.get(app.schedule_action_input) {
        let entry = Scheduled {
            index: app.schedule_action_input,
            id: action.id().to_owned(),
            tick: app.schedule_tick_input,
        };
        app.schedule = with_entry(&app.schedule, entry);
    }

    if !app.schedule.is_empty()
        && ui
            .button("Clear schedule")
            .on_hover_text("Remove all scheduled actions")
            .clicked()
    {
        app.schedule = Schedule::default();
    }
}

/// Adds the button that removes the scheduled action shown in its row as `entry`.
pub(super) fn remove_button(ui: &mut egui::Ui, entry: &str) -> egui::Response {
    icon_button(ui, MDI_DELETE_OUTLINE, &format!("Remove {entry}"), "Remove action")
}

/// Returns `schedule` with `entry` after every entry due at or before its tick.
fn with_entry(schedule: &Schedule, entry: Scheduled) -> Schedule {
    let mut entries = schedule.entries().to_vec();
    let position = entries.partition_point(|existing| existing.tick <= entry.tick);
    entries.insert(position, entry);
    Schedule::from_entries(entries)
}

/// Returns `schedule` without the entry at `position`.
fn without_entry(schedule: &Schedule, position: usize) -> Schedule {
    let mut entries = schedule.entries().to_vec();
    if position < entries.len() {
        entries.remove(position);
    }
    Schedule::from_entries(entries)
}

#[cfg(test)]
mod tests {
    use henad_core::action::{Schedule, Scheduled};

    use super::{with_entry, without_entry};

    #[test]
    fn an_added_action_follows_every_entry_due_at_or_before_its_tick() {
        let entry = |id: &str, tick| Scheduled {
            index: 0,
            id: id.to_owned(),
            tick,
        };
        let ids = |schedule: &Schedule| -> Vec<String> { schedule.entries().iter().map(|e| e.id.clone()).collect() };

        let mut schedule = Schedule::default();
        for (id, tick) in [("a", 5), ("b", 1), ("c", 5), ("d", 0)] {
            schedule = with_entry(&schedule, entry(id, tick));
        }
        assert_eq!(ids(&schedule), ["d", "b", "a", "c"], "by tick, then in the order added");

        let schedule = without_entry(&schedule, 1);
        assert_eq!(ids(&schedule), ["d", "a", "c"]);
        assert_eq!(
            without_entry(&schedule, 3),
            schedule,
            "a position past the end removes nothing"
        );
    }
}
