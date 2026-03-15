use crate::{
    HenadApp,
    icons::material_design_icons::{MDI_PAUSE, MDI_PLAY, MDI_SKIP_NEXT},
};
use henad_core::params::{ParamKind, ParamValue};

#[expect(
    clippy::too_many_lines,
    reason = "UI function with auto-generated controls"
)]
pub fn sidebar_panel(ctx: &egui::Context, app: &mut HenadApp) {
    // Statistics on the right
    egui::SidePanel::right("stats_panel")
        .min_width(200.0)
        .show(ctx, |ui| {
            ui.heading("Statistics");
            if let Some(runner) = &app.runner {
                egui::Grid::new("stats_grid")
                    .num_columns(2)
                    .striped(true)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for stat in runner.state().stats() {
                            let [r, g, b, _] = stat.color;
                            let color = egui::Color32::from_rgb(r, g, b);
                            ui.colored_label(color, stat.label);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.colored_label(color, format!("{:.0}", stat.value));
                                },
                            );
                            ui.end_row();
                        }
                    });
            } else {
                ui.separator();
                ui.label("No simulation loaded.");
            }
            stats::stats_chart(ui, app);
        });

    egui::SidePanel::left("sidebar")
        .min_width(200.0)
        .show(ctx, |ui| {
            let mut do_reset = false;
            let mut do_offload = false;
            ui.horizontal(|ui| {
                let has_runner = app.runner.is_some();
                let is_running = app.runner.as_ref().is_some_and(|r| r.is_running());
                let icon = if is_running { MDI_PAUSE } else { MDI_PLAY };

                if ui
                    .add_enabled(has_runner, egui::Button::new(icon))
                    .on_hover_text("Play / Pause")
                    .clicked()
                {
                    if let Some(runner) = &mut app.runner {
                        runner.toggle();
                    }
                }
                if ui
                    .add_enabled(has_runner, egui::Button::new(MDI_SKIP_NEXT))
                    .on_hover_text("Step")
                    .clicked()
                {
                    if let Some(runner) = &mut app.runner {
                        runner.step_once();
                    }
                }
                ui.separator();
                if ui.button("Reset").clicked() {
                    do_reset = true;
                }

                ui.separator();

                if ui.button("Load Model").clicked() {
                    app.reset_simulation();
                }
                if ui
                    .add_enabled(has_runner, egui::Button::new("Offload"))
                    .on_hover_text("Free simulation memory")
                    .clicked()
                {
                    do_offload = true;
                }
            });
            if do_reset {
                app.reset_simulation();
            }
            if do_offload {
                app.offload_simulation();
            }

            ui.checkbox(&mut app.rendering_enabled, "Rendering");

            // TPS controls
            {
                let uncapped_changed = ui.checkbox(&mut app.uncapped, "Unlimited TPS").changed();

                if !app.uncapped {
                    let tps_changed = ui
                        .add(
                            egui::Slider::new(&mut app.target_tps, 1.0..=1000.0)
                                .logarithmic(true)
                                .text("Target TPS"),
                        )
                        .changed();
                    if tps_changed {
                        if let Some(runner) = &mut app.runner {
                            runner.set_target_tps(app.target_tps);
                        }
                    }
                }

                if uncapped_changed {
                    if let Some(runner) = &mut app.runner {
                        runner.set_uncapped(app.uncapped);
                    }
                }
            }

            if let Some(runner) = &mut app.runner {
                let mut max = runner.max_steps_per_frame() as i32;
                if ui
                    .add(egui::Slider::new(&mut max, 1..=1000).logarithmic(true).text("Max steps/frame"))
                    .changed()
                {
                    runner.set_max_steps_per_frame(max as u32);
                }
            }

            {
                let mut cap = app.history_capacity;
                if ui
                    .add(
                        egui::Slider::new(&mut cap, 100..=1_000_000)
                            .logarithmic(true)
                            .text("History length"),
                    )
                    .changed()
                {
                    app.history_capacity = cap;
                    if let Some(runner) = &mut app.runner {
                        runner.state_mut().resize_history(cap);
                    }
                }
            }

            ui.separator();
            ui.heading("Model");

            // Model selector
            let model_names: Vec<&str> = app.registry.iter().map(|m| m.name.as_str()).collect();
            let mut changed_model = false;
            egui::ComboBox::from_label("Select Model")
                .selected_text(
                    model_names
                        .get(app.selected_model)
                        .copied()
                        .unwrap_or("None"),
                )
                .show_ui(ui, |ui| {
                    for (i, name) in model_names.iter().enumerate() {
                        if ui
                            .selectable_value(&mut app.selected_model, i, *name)
                            .changed()
                        {
                            changed_model = true;
                        }
                    }
                });

            if changed_model {
                if let Some(entry) = app.registry.get(app.selected_model) {
                    app.param_values = entry
                        .param_descriptors
                        .iter()
                        .map(|p| p.kind.default_value())
                        .collect();
                }
            }

            ui.separator();

            // Auto-generated parameter controls
            let descriptors: Vec<_> = app
                .registry
                .get(app.selected_model)
                .map(|m| m.param_descriptors.clone())
                .unwrap_or_default();

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
                            .add(
                                egui::Slider::new(&mut v_i32, *min as i32..=*max as i32)
                                    .text(desc.label),
                            )
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

            // Apply live parameter changes to running state
            for (idx, val) in &param_changed {
                if let Some(runner) = &mut app.runner {
                    runner.state_mut().set_param(*idx, val);
                }
            }
        });
}

mod stats {
    use crate::HenadApp;

    pub fn stats_chart(ui: &mut egui::Ui, app: &HenadApp) {
        let Some(runner) = &app.runner else {
            return;
        };

        let history = runner.state().stats_history();
        if history.is_empty() {
            return;
        }

        egui_plot::Plot::new("stats_plot")
            .height(200.0)
            .show_axes(true)
            .legend(egui_plot::Legend::default())
            .show(ui, |plot_ui| {
                let filled = history.len();
                let oldest_tick = history.oldest_tick();

                for (col, desc) in history.descriptors().iter().enumerate() {
                    let [r, g, b, _] = desc.color;
                    let color = egui::Color32::from_rgb(r, g, b);

                    let points: Vec<[f64; 2]> = (0..filled)
                        .filter_map(|j| {
                            let val = history.get(col, j)?;
                            Some([(oldest_tick + j) as f64, val])
                        })
                        .collect();

                    plot_ui.line(
                        egui_plot::Line::new(desc.label, points)
                            .color(color)
                            .width(1.5),
                    );
                }
            });
    }
}
