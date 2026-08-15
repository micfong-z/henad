//! Parameter widgets generated from the model's `ParamDescriptor`s.

use crate::icons::material_design_icons::{MDI_ALERT, MDI_INFORMATION, MDI_RESTART};
use crate::state::AppState;
use crate::ui::banner;
use henad_compute::cpu::sim_thread::SimCommand;
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};

pub fn params_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let descriptors: Vec<_> = app
        .registry
        .get(app.selected_model)
        .map(|m| m.param_descriptors.clone())
        .unwrap_or_default();

    if descriptors.is_empty() {
        ui.label("This model has no parameters.");
        return;
    }

    // Before the sliders draw: a long slider label widens the region behind it, and the footer
    // would then wrap against that width and be clipped.
    let panel_width = ui.available_width();

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
                let mut slider = egui::Slider::new(v, *min..=*max).text(text);
                if let Some(s) = step {
                    slider = slider.step_by(f64::from(*s));
                }
                if with_hint(ui.add(slider), hint).changed() {
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

    for (idx, val) in &param_changed {
        // Reload-only parameters are rejected by the running state anyway, so remember the edit
        // instead of sending it and having the runner complain.
        if !descriptors[*idx].is_live() {
            if let Some(mark) = app.pending_reload.get_mut(*idx) {
                *mark = true;
            }
            continue;
        }
        if sim_matches && let Some(thread) = &mut app.sim_thread {
            thread.send(SimCommand::SetParam {
                index: *idx,
                value: val.clone(),
            });
        }
    }

    notice(ui, app, &descriptors, panel_width);
}

/// True when `index` has been edited to a value the running sim will not pick up on its own.
fn is_pending_reload(app: &AppState, index: usize, desc: &ParamDescriptor) -> bool {
    !desc.is_live() && app.selection_is_loaded() && app.pending_reload.get(index) == Some(&true)
}

/// Widget label, marked and coloured when the parameter needs a reload.
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

fn with_hint(response: egui::Response, hint: Option<&str>) -> egui::Response {
    match hint {
        Some(hint) => response.on_hover_text(hint),
        None => response,
    }
}

/// Information on the parameter state, if any, that the user should be aware of.
fn notice(ui: &mut egui::Ui, app: &AppState, descriptors: &[ParamDescriptor], width: f32) {
    let pending_count = descriptors
        .iter()
        .enumerate()
        .filter(|(i, desc)| is_pending_reload(app, *i, desc))
        .count();
    let shortfalls = app.selection_shortfalls();

    let error = ui.visuals().error_fg_color;
    let warn = ui.visuals().warn_fg_color;
    let plain = ui.visuals().text_color();

    let (icon, colour, title, detail) = if !shortfalls.is_empty() {
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
        let running = app
            .loaded_model
            .and_then(|i| app.registry.get(i))
            .map_or("Another model", |entry| entry.name.as_str());
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
            format!("{pending_count} changed parameter{plural} {verb} effect after {MDI_RESTART}\u{a0}Build."),
        )
    } else {
        return;
    };

    ui.add_space(8.0);
    ui.scope(|ui| {
        ui.set_max_width(width);
        ui.separator();
        banner(ui, icon, colour, title, &detail);
    });
}
