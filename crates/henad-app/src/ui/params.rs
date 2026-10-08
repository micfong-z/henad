//! Parameter widgets generated from the model's `ParamDescriptor`s.

use std::num::ParseIntError;

use crate::icons::material_design_icons::{
    MDI_ALERT, MDI_DELETE_OUTLINE, MDI_DICE_5, MDI_INFORMATION, MDI_PLUS, MDI_RESTART,
};
use crate::state::AppState;
use crate::ui::banner;
use crate::ui::sweep::layout::{add_button, icon_button};
use henad_compute::cpu::sim_thread::SimCommand;
use henad_core::action::{ActionDescriptor, Schedule, Scheduled};
use henad_core::authoring::primitives::rng::mix_seed;
use henad_core::explore::value::format_value;
use henad_core::params::{ParamDescriptor, ParamFormat, ParamKind, ParamValue};
use web_time::{SystemTime, UNIX_EPOCH};

/// Line shown under the Seed field while its text is not a seed.
pub const INVALID_SEED: &str = "Seed must be an integer from 0 to 18446744073709551615.";

/// Line the panel shows instead of its rows while no model is selected.
const NO_MODEL_SELECTED: &str = "No model selected.";

/// Space between the Seed field's frame and its text, egui's default for a text field.
const SEED_FIELD_MARGIN: egui::Margin = egui::Margin::symmetric(4, 2);

pub fn params_ui(ui: &mut egui::Ui, app: &mut AppState) {
    // With nothing selected, a seed or a schedule would apply to no build.
    let Some(entry) = app.selected_entry() else {
        ui.label(NO_MODEL_SELECTED);
        return;
    };
    let descriptors = entry.param_descriptors().to_vec();

    // Drawn before the sliders. A long slider label widens the region behind it, and the footer
    // would then wrap against that width and be clipped.
    let panel_width = ui.available_width();

    seed_row(ui, app);

    if descriptors.is_empty() {
        ui.label("This model has no parameters.");
        actions_ui(ui, app);
        notice(ui, app, &descriptors, panel_width);
        return;
    }

    let pending: Vec<bool> = descriptors
        .iter()
        .enumerate()
        .map(|(i, desc)| is_pending_reload(app, i, desc))
        .collect();

    let sim_matches = app.selection_is_loaded();
    let mut param_changed = Vec::new();

    let reload_hint = format!(
        "This parameter is only read when the model is built. Press {MDI_RESTART}\u{a0}Build to apply after change."
    );
    let pending_hint =
        format!("This parameter has been changed but not applied. Press {MDI_RESTART}\u{a0}Build to apply.");

    for (i, desc) in descriptors.iter().enumerate() {
        let hint = if desc.is_live() {
            None
        } else if pending[i] {
            Some(pending_hint.as_str())
        } else {
            Some(reload_hint.as_str())
        };
        let text = param_text(ui, desc, pending[i]);

        let Some(val) = app.param_values.get_mut(i) else {
            continue;
        };

        match (&desc.kind, val) {
            (ParamKind::F32 { min, max, step, .. }, ParamValue::F32(v)) => {
                let before = *v;
                let mut slider = f32_slider(v, *min, *max, *step).text(text);
                if desc.format == ParamFormat::Percent {
                    slider = as_percent(slider, *step);
                }
                with_hint(ui.add(slider), hint);
                if *v != before {
                    param_changed.push((i, ParamValue::F32(*v)));
                }
            }
            (ParamKind::U32 { min, max, .. }, ParamValue::U32(v)) => {
                let mut v_i32 = *v as i32;
                let slider = egui::Slider::new(&mut v_i32, *min as i32..=*max as i32).text(text);
                if with_hint(ui.add(slider), hint).changed() {
                    *v = v_i32 as u32;
                    param_changed.push((i, ParamValue::U32(*v)));
                }
            }
            (ParamKind::Bool { .. }, ParamValue::Bool(v)) => {
                if with_hint(ui.checkbox(v, text), hint).changed() {
                    param_changed.push((i, ParamValue::Bool(*v)));
                }
            }
            (ParamKind::Choice { options, .. }, ParamValue::Choice(v)) => {
                let combo = egui::ComboBox::from_label(text)
                    .selected_text(options.get(*v).copied().unwrap_or("?"))
                    .show_ui(ui, |ui| {
                        for (j, opt) in options.iter().enumerate() {
                            if ui.selectable_value(v, j, *opt).changed() {
                                param_changed.push((i, ParamValue::Choice(*v)));
                            }
                        }
                    });
                with_hint(combo.response, hint);
            }
            _ => {}
        }
    }

    let mut sent_live = false;
    for (idx, val) in &param_changed {
        // The running state rejects a reload-only parameter, so the edit is only marked pending.
        if !descriptors[*idx].is_live() {
            if let Some(mark) = app.pending_reload.get_mut(*idx) {
                *mark = true;
            }
            continue;
        }
        if sim_matches && app.send_live_param(*idx, val.clone()) {
            sent_live = true;
        }
    }
    if sent_live {
        app.mark_opened_run_modified();
    }

    actions_ui(ui, app);
    notice(ui, app, &descriptors, panel_width);
}

/// Draws the Seed field and its dice button, with an error line while [`parse_seed`] rejects the field's text.
fn seed_row(ui: &mut egui::Ui, app: &mut AppState) {
    let pending = app.seed_pending();
    let hint = if pending {
        format!("Seed changed. Press {MDI_RESTART}\u{a0}Build to apply.")
    } else {
        "Seed for random number generation.".to_owned()
    };
    let mut label = egui::RichText::new(format!("Seed {MDI_RESTART}"));
    if pending {
        label = label.color(ui.visuals().warn_fg_color);
    }
    let width = seed_field_width(ui);

    ui.horizontal(|ui| {
        let field = egui::TextEdit::singleline(&mut app.seed_text)
            .hint_text("Default")
            .margin(SEED_FIELD_MARGIN)
            .desired_width(width);
        let field = ui.add(field).on_hover_text(hint.as_str());
        if field.changed()
            && let Ok(seed) = parse_seed(&app.seed_text)
        {
            app.seed = seed;
        }
        if dice_button(ui).clicked() {
            let seed = draw_seed(app.seed);
            app.seed = Some(seed);
            app.seed_text = seed.to_string();
        }
        let label = ui.label(label).on_hover_text(hint.as_str());
        field.labelled_by(label.id);
    });

    if parse_seed(&app.seed_text).is_err() {
        ui.colored_label(ui.visuals().error_fg_color, INVALID_SEED);
    }
}

/// Adds the button beside the Seed field that draws a random seed.
fn dice_button(ui: &mut egui::Ui) -> egui::Response {
    add_button(ui, true, egui::Button::new(MDI_DICE_5), "Generate random seed").on_hover_text("Generate random seed")
}

/// Returns the width of a Seed field that shows every digit of the largest seed.
pub(crate) fn seed_field_width(ui: &egui::Ui) -> f32 {
    let font = egui::FontSelection::Default.resolve(ui.style());
    let digits = ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(u64::MAX.to_string(), font, egui::Color32::PLACEHOLDER)
            .size()
            .x
    });
    digits + SEED_FIELD_MARGIN.sum().x + ui.visuals().text_cursor.stroke.width
}

/// Returns the seed in the Seed field's `text`, `None` for an empty field.
///
/// `None` stands for the model's default seed. No number equals it.
///
/// # Errors
///
/// Returns the parse error for text that is not a whole number from 0 to `u64::MAX`.
pub fn parse_seed(text: &str) -> Result<Option<u64>, ParseIntError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse().map(Some)
}

/// Returns a seed drawn from the clock, mixed with `previous` so two draws in one clock tick differ.
pub(crate) fn draw_seed(previous: Option<u64>) -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos() as u64);
    mix_seed(nanos ^ previous.unwrap_or(0))
}

/// Draws a button per action the model declares, under the parameter widgets, then the scheduled actions.
///
/// A press goes to the running sim. An action changes state that exists only once the model is built.
fn actions_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let actions: Vec<ActionDescriptor> = app
        .selected_entry()
        .map(|entry| entry.action_descriptors().to_vec())
        .unwrap_or_default();
    if actions.is_empty() {
        return;
    }

    let loaded = app.selection_is_loaded();
    let mut pressed = None;

    ui.add_space(8.0);
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        for (i, action) in actions.iter().enumerate() {
            if ui
                .add_enabled(loaded, egui::Button::new(action.label))
                .on_disabled_hover_text(format!(
                    "Press {MDI_RESTART}\u{a0}Build to run this model, then {}",
                    action.label.to_lowercase()
                ))
                .clicked()
            {
                pressed = Some(i);
            }
        }
    });

    if let Some(index) = pressed
        && let Some(thread) = &mut app.sim_thread
    {
        thread.send(SimCommand::Act(index));
        app.mark_opened_run_modified();
    }

    schedule_ui(ui, app, &actions);
}

/// Draws the actions each build runs at their ticks, with a row to add an action.
fn schedule_ui(ui: &mut egui::Ui, app: &mut AppState, actions: &[ActionDescriptor]) {
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
            .map_or(entry.id.as_str(), |action| action.label);
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
        let selected = actions.get(app.schedule_action_input).map_or("", |action| action.label);
        egui::ComboBox::from_id_salt("scheduled_action")
            .selected_text(selected)
            .show_ui(ui, |ui| {
                for (index, action) in actions.iter().enumerate() {
                    ui.selectable_value(&mut app.schedule_action_input, index, action.label);
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
            id: action.id.to_owned(),
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
fn remove_button(ui: &mut egui::Ui, entry: &str) -> egui::Response {
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

/// Returns whether parameter `index` has been edited to a value the running sim will not pick up on its own.
fn is_pending_reload(app: &AppState, index: usize, desc: &ParamDescriptor) -> bool {
    !desc.is_live() && app.selection_is_loaded() && app.pending_reload.get(index) == Some(&true)
}

/// Returns the widget label, marked when the parameter needs a reload and coloured while an edit of it is pending.
fn param_text(ui: &egui::Ui, desc: &ParamDescriptor, pending: bool) -> egui::RichText {
    if desc.is_live() {
        return egui::RichText::new(desc.label);
    }
    let text = egui::RichText::new(format!("{} {MDI_RESTART}", desc.label));
    if pending {
        text.color(ui.visuals().warn_fg_color)
    } else {
        text
    }
}

/// Returns a slider over `value` from `min` to `max`, snapping to `step`.
///
/// Note that the slider snaps `value` only when the user edits it. Drawing it leaves a value set by a spec or an
/// opened run as it is.
fn f32_slider(value: &mut f32, min: f32, max: f32, step: Option<f32>) -> egui::Slider<'_> {
    let range = decimal_value(min)..=decimal_value(max);
    let slider = egui::Slider::from_get_set(range, move |edited| {
        if let Some(edited) = edited {
            *value = edited as f32;
        }
        f64::from(*value)
    })
    .clamping(egui::SliderClamping::Edits);
    match step {
        Some(step) => slider.step_by(decimal_value(step)),
        None => slider,
    }
}

/// Returns `value` widened to `f64` through its shortest decimal form.
///
/// A slider snaps to multiples of its step in `f64`. A step of `0.01_f32` widened in binary would snap 0.05 to the
/// `f32` below it.
fn decimal_value(value: f32) -> f64 {
    value.to_string().parse().unwrap_or(f64::from(value))
}

/// Returns `value` as the Parameters panel shows it, a fraction as a percentage.
pub(crate) fn display_value(descriptor: &ParamDescriptor, value: &ParamValue) -> String {
    match (&descriptor.kind, value) {
        (ParamKind::F32 { step, .. }, ParamValue::F32(number)) if descriptor.format == ParamFormat::Percent => {
            percent_text(f64::from(*number), percent_decimals(*step))
        }
        _ => format_value(&descriptor.kind, value),
    }
}

/// Returns the fraction `fraction` as a percentage with `decimals` decimals.
fn percent_text(fraction: f64, decimals: usize) -> String {
    format!("{:.decimals$}%", fraction * 100.0)
}

/// Returns `slider` showing and reading its fraction as a percentage.
fn as_percent(slider: egui::Slider<'_>, step: Option<f32>) -> egui::Slider<'_> {
    let decimals = percent_decimals(step);
    slider
        .custom_formatter(move |value, _| percent_text(value, decimals))
        .custom_parser(|text| {
            let number = text.trim().trim_end_matches('%').trim_end();
            number.parse::<f64>().ok().map(|percent| percent / 100.0)
        })
}

/// Returns the smallest number of decimals that shows each step as a distinct percentage.
fn percent_decimals(step: Option<f32>) -> usize {
    let Some(step) = step else {
        return 1;
    };
    let percent = f64::from(step) * 100.0;
    (0..4)
        .find(|&places| {
            let scaled = percent * 10f64.powi(places);
            (scaled - scaled.round()).abs() < 1e-6
        })
        .map_or(4, |places| places as usize)
}

fn with_hint(response: egui::Response, hint: Option<&str>) -> egui::Response {
    match hint {
        Some(hint) => response.on_hover_text(hint),
        None => response,
    }
}

/// Draws a banner on the state of the parameters when one applies: a size the device cannot hold, no simulation,
/// another model loaded, or edits awaiting a build.
fn notice(ui: &mut egui::Ui, app: &AppState, descriptors: &[ParamDescriptor], width: f32) {
    let pending_count = descriptors
        .iter()
        .enumerate()
        .filter(|(i, desc)| is_pending_reload(app, *i, desc))
        .count()
        + usize::from(app.seed_pending())
        + usize::from(app.schedule_pending());
    let shortfalls = app.selection_shortfalls();

    let error = ui.visuals().error_fg_color;
    let warn = ui.visuals().warn_fg_color;
    let plain = ui.visuals().text_color();

    let (icon, color, title, detail) = if !shortfalls.is_empty() {
        (
            MDI_ALERT,
            error,
            "Too large for this device",
            format!("{}. Reduce the size parameters to build.", shortfalls.join(". ")),
        )
    } else if app.sim_thread.is_none() {
        (
            MDI_INFORMATION,
            plain,
            "No simulation loaded",
            format!("Parameters will be applied after {MDI_RESTART}\u{a0}Build."),
        )
    } else if !app.selection_is_loaded() {
        let running = app.loaded_entry().map_or("Another model", |entry| entry.name());
        (
            MDI_ALERT,
            warn,
            "Selected model not loaded",
            format!(
                "The parameters above do not apply to {running}. Press {MDI_RESTART}\u{a0}Build to switch to the selected model."
            ),
        )
    } else if pending_count > 0 {
        let (plural, verb) = if pending_count == 1 {
            ("", "takes")
        } else {
            ("s", "take")
        };
        (
            MDI_ALERT,
            warn,
            "Reload needed",
            format!("{pending_count} change{plural} {verb} effect after {MDI_RESTART}\u{a0}Build."),
        )
    } else {
        return;
    };

    ui.add_space(8.0);
    ui.scope(|ui| {
        ui.set_max_width(width);
        ui.separator();
        banner(ui, icon, color, title, &detail);
    });
}

#[cfg(test)]
mod tests {
    use henad_compute::entry::ModelSet;
    use henad_core::action::{Schedule, Scheduled};
    use henad_core::explore::value::parse_value;
    use henad_core::metadata::Backend;
    use henad_core::params::{ParamKind, ParamValue};
    use henad_models::example_models;

    use super::{
        INVALID_SEED, NO_MODEL_SELECTED, decimal_value, dice_button, display_value, f32_slider, params_ui, parse_seed,
        percent_decimals, remove_button, with_entry, without_entry,
    };
    use crate::icons::material_design_icons::{MDI_DELETE_OUTLINE, MDI_DICE_5};
    use crate::state::AppState;

    /// Draws the slider of an F32 parameter of `kind` over `value` for one frame of `context`, focused and pressed
    /// with `keys`, and returns whether it reports a change.
    fn draw_slider(context: &egui::Context, kind: &ParamKind, value: &mut f32, keys: &[egui::Key]) -> bool {
        let ParamKind::F32 { min, max, step, .. } = *kind else {
            panic!("an F32 parameter");
        };
        let events = keys
            .iter()
            .map(|&key| egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .collect();
        let input = egui::RawInput {
            events,
            ..egui::RawInput::default()
        };
        let mut changed = false;
        let output = context.run_ui(input, |ui| {
            let response = ui.add(f32_slider(value, min, max, step));
            // A request resets the focus lock that keeps the arrow keys on the slider.
            if !response.has_focus() {
                response.request_focus();
            }
            changed |= response.changed();
        });
        output.drop_without_applying_deltas();
        changed
    }

    #[test]
    fn a_slider_leaves_a_value_alone_until_it_is_edited() {
        let models = example_models();
        let sliders = models.iter().flat_map(|entry| {
            entry
                .param_descriptors()
                .iter()
                .map(move |descriptor| (entry.id(), descriptor))
        });
        let mut checked = 0;
        for (model, descriptor) in sliders {
            let ParamKind::F32 {
                default,
                min,
                max,
                step,
            } = descriptor.kind
            else {
                continue;
            };
            let context = egui::Context::default();
            let mut value = default;
            for _ in 0..2 {
                let changed = draw_slider(&context, &descriptor.kind, &mut value, &[]);
                assert!(!changed, "{model} {}: drawing reports a change", descriptor.id);
                assert_eq!(value.to_bits(), default.to_bits(), "{model} {}: drawn", descriptor.id);
            }
            let Some(step) = step else {
                continue;
            };
            // A step there and back lands on the value it left, when that value is on the slider's grid.
            let offset = (decimal_value(default) - decimal_value(min)) / decimal_value(step);
            if (offset - offset.round()).abs() > 1e-9 {
                continue;
            }
            let (there, back) = if default + step <= max {
                (egui::Key::ArrowRight, egui::Key::ArrowLeft)
            } else {
                (egui::Key::ArrowLeft, egui::Key::ArrowRight)
            };
            assert!(draw_slider(&context, &descriptor.kind, &mut value, &[there]));
            assert_ne!(value, default, "{model} {}: the key moves the slider", descriptor.id);
            assert!(draw_slider(&context, &descriptor.kind, &mut value, &[back]));
            assert_eq!(
                value.to_bits(),
                default.to_bits(),
                "{model} {}: {value} after a step there and back",
                descriptor.id
            );
            checked += 1;
        }
        assert!(checked > 0, "no model declares a stepped F32 parameter");
    }

    #[test]
    fn a_stepped_value_is_the_one_a_spec_writes() {
        let models = example_models();
        let sir = models.get("sir").expect("SIR is registered");
        let descriptor = sir
            .param_descriptors()
            .iter()
            .find(|descriptor| descriptor.id == "recovery_rate")
            .expect("SIR declares a recovery rate");
        let context = egui::Context::default();
        let mut value = 0.04;
        for keys in [&[][..], &[], &[egui::Key::ArrowRight]] {
            draw_slider(&context, &descriptor.kind, &mut value, keys);
        }
        assert_eq!(
            Ok(ParamValue::F32(value)),
            parse_value(&descriptor.kind, "0.05"),
            "0.04 and one step is 0.05"
        );
    }

    #[test]
    fn a_percentage_shows_as_the_panel_writes_it() {
        let models = example_models();
        let sir = models.get("sir").expect("SIR is registered");
        let percent = sir
            .param_descriptors()
            .iter()
            .find(|descriptor| descriptor.id == "initial_infected_pct")
            .expect("SIR declares its initially infected share");
        let rate = sir
            .param_descriptors()
            .iter()
            .find(|descriptor| descriptor.id == "infection_rate")
            .expect("SIR declares an infection rate");
        assert_eq!(display_value(percent, &ParamValue::F32(0.01)), "1.0%");
        assert_eq!(display_value(rate, &ParamValue::F32(0.3)), "0.3");
    }

    #[test]
    fn an_empty_seed_field_is_the_default_and_anything_else_a_whole_number() {
        assert_eq!(parse_seed(""), Ok(None), "empty");
        assert_eq!(parse_seed("  "), Ok(None), "blank");
        assert_eq!(parse_seed(" 42 "), Ok(Some(42)), "padded");
        assert_eq!(parse_seed("0"), Ok(Some(0)), "zero parses as seed 0");
        assert_eq!(parse_seed("18446744073709551615"), Ok(Some(u64::MAX)), "max u64");
        assert!(parse_seed("-1").is_err(), "negative");
        assert!(parse_seed("abc").is_err(), "text");
        assert!(parse_seed("1.5").is_err(), "fraction");
        assert!(parse_seed("18446744073709551616").is_err(), "overflow");
        assert!(
            INVALID_SEED.contains(&u64::MAX.to_string()),
            "the error names the largest seed"
        );
    }

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

    #[test]
    fn a_percentage_shows_as_many_decimals_as_its_step_needs() {
        assert_eq!(percent_decimals(Some(0.01)), 0, "1% steps");
        assert_eq!(percent_decimals(Some(0.001)), 1, "0.1% steps");
        assert_eq!(percent_decimals(Some(0.005)), 1, "0.5% steps");
        assert_eq!(percent_decimals(Some(0.0025)), 2, "0.25% steps");
        assert_eq!(percent_decimals(None), 1, "no step");
    }

    /// Returns the accessible names of the widgets `add` draws in one frame.
    fn accessible_names(add: impl FnMut(&mut egui::Ui)) -> Vec<String> {
        accessible_texts(add, |node| node.label())
    }

    /// Returns the text that `read` extracts from each accessible node that `add` draws in one frame.
    fn accessible_texts(
        add: impl FnMut(&mut egui::Ui),
        read: impl Fn(&egui::accesskit::Node) -> Option<&str>,
    ) -> Vec<String> {
        let context = egui::Context::default();
        context.enable_accesskit();
        let output = context.run_ui(egui::RawInput::default(), add);
        let texts = output
            .platform_output
            .accesskit_update
            .as_ref()
            .map(|update| {
                update
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| read(node).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        output.drop_without_applying_deltas();
        texts
    }

    /// With no model selected, the panel shows neither a Seed field nor a claim that the model has no parameters.
    #[test]
    fn with_no_model_selected_the_panel_offers_nothing_to_edit() {
        let mut models = ModelSet::new(henad_core::build_info!());
        for entry in &example_models() {
            if entry.metadata().backend == Backend::Gpu {
                models.insert(entry.clone()).expect("example ids are unique");
            }
        }
        let Some(mut app) = AppState::headless(models, false) else {
            return;
        };
        assert!(app.selected_entry().is_none());
        let texts = accessible_texts(
            |ui| params_ui(ui, &mut app),
            |node| node.value().or_else(|| node.label()),
        );
        assert!(texts.iter().any(|text| text == NO_MODEL_SELECTED), "{texts:?}");
        assert!(
            !texts
                .iter()
                .any(|text| text == "Generate random seed" || text.contains("no parameters")),
            "{texts:?}"
        );
    }

    /// The dice and remove buttons carry names a screen reader can read, instead of their icon glyphs.
    #[test]
    fn the_icon_buttons_are_named_for_what_they_do() {
        let names = accessible_names(|ui| {
            dice_button(ui);
            remove_button(ui, "Seed outbreak at tick 5");
        });
        assert!(names.iter().any(|name| name == "Generate random seed"), "{names:?}");
        assert!(
            names.iter().any(|name| name == "Remove Seed outbreak at tick 5"),
            "{names:?}"
        );
        assert!(
            !names
                .iter()
                .any(|name| name == MDI_DICE_5 || name == MDI_DELETE_OUTLINE),
            "{names:?}"
        );
    }
}
