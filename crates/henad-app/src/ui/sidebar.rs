use crate::{
    HenadApp,
    icons::material_design_icons::{
        MDI_ARROW_TOP_RIGHT_THIN, MDI_CHART_HISTOGRAM, MDI_CIRCLE_SMALL, MDI_PAUSE, MDI_PLAY,
        MDI_SKIP_NEXT,
    },
};
use henad_compute::sim_thread::SimCommand;
use henad_core::params::{ParamKind, ParamValue};
use henad_core::view::StatValue;

#[expect(
    clippy::too_many_lines,
    reason = "UI function"
)]
pub fn sidebar_panel(ctx: &egui::Context, app: &mut HenadApp) {
    // Statistics on the right
    egui::SidePanel::right("stats_panel")
        .min_width(200.0)
        .show(ctx, |ui| {
            ui.heading("Statistics");
            if let Some(snap) = &app.snapshot {
                egui::Grid::new("stats_grid")
                    .num_columns(2)
                    .striped(true)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for stat in &snap.stats {
                            let [r, g, b, _] = stat.color;
                            let color = egui::Color32::from_rgb(r, g, b);
                            ui.colored_label(color, stat.label);
                            ui.horizontal(|ui| {
                                let (text, data_type) = match &stat.value {
                                    StatValue::Scalar(v) => (format!("{v:.0}"), "Scalar"),
                                    StatValue::Vector2D { x, y } => {
                                        let mag = x.hypot(*y);
                                        (format!("({x:.1}, {y:.1}) |{mag:.1}|"), "Vector2D")
                                    }
                                    StatValue::Histogram { counts, .. } => {
                                        let total: u64 = counts.iter().sum();
                                        (format!("n={total}"), "Histogram")
                                    }
                                };

                                let icon = match &stat.value {
                                    StatValue::Scalar(_) => MDI_CIRCLE_SMALL,
                                    StatValue::Vector2D { .. } => MDI_ARROW_TOP_RIGHT_THIN,
                                    StatValue::Histogram { .. } => MDI_CHART_HISTOGRAM,
                                };
                                ui.colored_label(egui::Color32::GRAY, icon)
                                    .on_hover_text(data_type);
                                ui.colored_label(color, text);
                            });
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
                let has_thread = app.sim_thread.is_some();
                let icon = if app.sim_running { MDI_PAUSE } else { MDI_PLAY };

                if ui
                    .add_enabled(has_thread, egui::Button::new(icon))
                    .on_hover_text("Play / Pause")
                    .clicked()
                {
                    if let Some(thread) = &mut app.sim_thread {
                        if app.sim_running {
                            thread.pause();
                        } else {
                            thread.play();
                        }
                        app.sim_running = !app.sim_running;
                    }
                }
                if ui
                    .add_enabled(has_thread, egui::Button::new(MDI_SKIP_NEXT))
                    .on_hover_text("Step")
                    .clicked()
                {
                    if let Some(thread) = &mut app.sim_thread {
                        thread.step_once();
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
                    .add_enabled(has_thread, egui::Button::new("Offload"))
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
                        if let Some(thread) = &mut app.sim_thread {
                            thread.send(SimCommand::SetTargetTps(app.target_tps));
                        }
                    }
                }

                if uncapped_changed {
                    if let Some(thread) = &mut app.sim_thread {
                        thread.send(SimCommand::SetUncapped(app.uncapped));
                    }
                }
            }

            // Max steps per frame
            {
                let mut max = 1i32;
                if ui
                    .add(
                        egui::Slider::new(&mut max, 1..=1000)
                            .logarithmic(true)
                            .text("Max steps/frame"),
                    )
                    .changed()
                {
                    if let Some(thread) = &mut app.sim_thread {
                        thread.send(SimCommand::SetMaxStepsPerFrame(max as u32));
                    }
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
                    if let Some(history) = &mut app.stats_history {
                        history.resize(cap);
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

            // Apply live parameter changes
            for (idx, val) in &param_changed {
                if let Some(thread) = &mut app.sim_thread {
                    thread.send(SimCommand::SetParam {
                        index: *idx,
                        value: val.clone(),
                    });
                }
            }
        });
}

mod stats {
    use crate::HenadApp;
    use henad_core::view::StatValue;

    pub fn stats_chart(ui: &mut egui::Ui, app: &HenadApp) {
        let Some(history) = &app.stats_history else {
            return;
        };

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

        // Render histogram bar charts for any Histogram stats in the latest snapshot
        if let Some(snap) = &app.snapshot {
            for stat in &snap.stats {
                if let StatValue::Histogram { edges, counts } = &stat.value {
                    histogram_chart(ui, stat.label, edges, counts, stat.color);
                }
            }
        }
    }

    fn histogram_chart(
        ui: &mut egui::Ui,
        label: &str,
        edges: &[f64],
        counts: &[u64],
        color: [u8; 4],
    ) {
        if edges.len() < 2 || counts.is_empty() {
            return;
        }
        let [r, g, b, _] = color;
        let bar_color = egui::Color32::from_rgb(r, g, b);

        ui.separator();
        ui.label(label);

        egui_plot::Plot::new(format!("hist_{label}"))
            .height(120.0)
            .show_axes(true)
            .show(ui, |plot_ui| {
                let bars: Vec<egui_plot::Bar> = edges
                    .windows(2)
                    .zip(counts.iter())
                    .map(|(edge_pair, &count)| {
                        let center = (edge_pair[0] + edge_pair[1]) * 0.5;
                        let width = edge_pair[1] - edge_pair[0];
                        egui_plot::Bar::new(center, count as f64).width(width)
                    })
                    .collect();

                plot_ui.bar_chart(egui_plot::BarChart::new(label, bars).color(bar_color));
            });
    }
}
