//! Parameter widgets generated from the model's `ParamDescriptor`s.

use crate::state::AppState;
use henad_compute::sim_thread::SimCommand;
use henad_core::params::{ParamKind, ParamValue};

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

    let mut param_changed = Vec::new();

    for (i, desc) in descriptors.iter().enumerate() {
        let Some(val) = app.param_values.get_mut(i) else {
            continue;
        };

        match (&desc.kind, val) {
            (ParamKind::F32 { min, max, step, .. }, ParamValue::F32(v)) => {
                let mut slider = egui::Slider::new(v, *min..=*max).text(desc.label);
                if let Some(s) = step {
                    slider = slider.step_by(f64::from(*s));
                }
                if ui.add(slider).changed() {
                    param_changed.push((i, ParamValue::F32(*v)));
                }
            }
            (ParamKind::U32 { min, max, .. }, ParamValue::U32(v)) => {
                let mut v_i32 = *v as i32;
                if ui
                    .add(egui::Slider::new(&mut v_i32, *min as i32..=*max as i32).text(desc.label))
                    .changed()
                {
                    *v = v_i32 as u32;
                    param_changed.push((i, ParamValue::U32(*v)));
                }
            }
            (ParamKind::Bool { .. }, ParamValue::Bool(v)) => {
                if ui.checkbox(v, desc.label).changed() {
                    param_changed.push((i, ParamValue::Bool(*v)));
                }
            }
            (ParamKind::Choice { options, .. }, ParamValue::Choice(v)) => {
                egui::ComboBox::from_label(desc.label)
                    .selected_text(options.get(*v).copied().unwrap_or("?"))
                    .show_ui(ui, |ui| {
                        for (j, opt) in options.iter().enumerate() {
                            if ui.selectable_value(v, j, *opt).changed() {
                                param_changed.push((i, ParamValue::Choice(*v)));
                            }
                        }
                    });
            }
            _ => {}
        }
    }

    for (idx, val) in &param_changed {
        if let Some(thread) = &mut app.sim_thread {
            thread.send(SimCommand::SetParam {
                index: *idx,
                value: val.clone(),
            });
        }
    }
}
