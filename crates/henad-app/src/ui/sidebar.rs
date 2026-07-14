use crate::{
    HenadApp,
    icons::material_design_icons::{
        MDI_ARROW_TOP_RIGHT_THIN, MDI_CHART_HISTOGRAM, MDI_CIRCLE_SMALL, MDI_PAUSE, MDI_PLAY, MDI_SKIP_NEXT,
    },
    ui::stats,
};
use henad_compute::sim_thread::SimCommand;
use henad_core::params::{ParamKind, ParamValue};
use henad_core::view::StatValue;

#[cfg(not(target_arch = "wasm32"))]
use crate::sim_runner::SimRunner;

#[expect(clippy::too_many_lines, reason = "UI function")]
pub fn sidebar_panel(ctx: &egui::Context, app: &mut HenadApp) {
    // Statistics on the right
    egui::SidePanel::right("stats_panel").min_width(200.0).show(ctx, |ui| {
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
                            ui.colored_label(egui::Color32::GRAY, icon).on_hover_text(data_type);
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

    egui::SidePanel::left("sidebar").min_width(200.0).show(ctx, |ui| {
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

            if ui.button("Load Model / Reset").clicked() {
                app.reset_simulation();
            }
            if ui
                .add_enabled(has_thread, egui::Button::new("Offload"))
                .on_hover_text("Free simulation memory")
                .clicked()
            {
                app.offload_simulation();
            }
        });

        ui.checkbox(&mut app.rendering_enabled, "Rendering");

        // A GPU model paces itself with the batch-size controller below and has no notion of
        // a TPS cap or a tick-based snapshot cadence, so the CPU pacing controls are swapped
        // out for the GPU ones rather than shown alongside them (they would be inert).
        #[cfg(not(target_arch = "wasm32"))]
        let is_gpu = app.sim_thread.as_ref().is_some_and(|t| t.gpu_stats().is_some());
        #[cfg(target_arch = "wasm32")]
        let is_gpu = false;

        if is_gpu {
            #[cfg(not(target_arch = "wasm32"))]
            gpu_batching_controls(ui, app);
        } else {
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
                let mut max = app.ticks_per_snapshot as i32;
                if ui
                    .add(
                        egui::Slider::new(&mut max, 1..=1000)
                            .logarithmic(true)
                            .text("Ticks/snapshot"),
                    )
                    .changed()
                {
                    app.ticks_per_snapshot = max as u32;
                    if let Some(thread) = &mut app.sim_thread {
                        thread.send(SimCommand::SetTicksPerSnapshot(app.ticks_per_snapshot));
                    }
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
            .selected_text(model_names.get(app.selected_model).copied().unwrap_or("None"))
            .show_ui(ui, |ui| {
                for (i, name) in model_names.iter().enumerate() {
                    if ui.selectable_value(&mut app.selected_model, i, *name).changed() {
                        changed_model = true;
                    }
                }
            });

        if changed_model {
            if let Some(entry) = app.registry.get(app.selected_model) {
                app.param_values = entry.param_descriptors.iter().map(|p| p.kind.default_value()).collect();
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

/// The GPU sim thread's batching controls, replacing the CPU pacing controls when a GPU-backed
/// model is active.
///
/// These are the still-needed controls from the old standalone GPU spike window, now backed by
/// real `HenadApp` fields instead of `ctx.data()` temp storage — the temp storage only existed
/// because the spike's panel was a free function with no `&mut self` to hang state off.
#[cfg(not(target_arch = "wasm32"))]
fn gpu_batching_controls(ui: &mut egui::Ui, app: &mut HenadApp) {
    use henad_compute::gpu::timing::MAX_BATCH_SIZE;

    let stats = app
        .sim_thread
        .as_ref()
        .and_then(crate::sim_runner::SimRunner::gpu_stats)
        .unwrap_or_default();

    ui.label(match stats.gpu_us_per_step {
        Some(us) => format!("GPU time/step: {us:.2} \u{b5}s"),
        None => "GPU time/step: N/A".to_owned(),
    });

    if ui
        .checkbox(&mut app.gpu_adaptive, "Adaptive batching")
        .on_hover_text(
            "Automatically pick steps-per-batch to keep each GPU submission under the target \
             time below, instead of a fixed batch size, so large batches don't block egui's own \
             rendering on the shared queue.",
        )
        .changed()
    {
        let adaptive = app.gpu_adaptive;
        if let Some(gpu) = app.sim_thread.as_mut().and_then(SimRunner::as_gpu_mut) {
            gpu.set_adaptive(adaptive);
        }
    }

    if app.gpu_adaptive {
        // Live batch size is the controller's output, not the (disabled) local slider value —
        // read it from stats every frame so it visibly tracks GPU cost.
        let mut live_batch_size = stats.batch_size;
        ui.add_enabled(
            false,
            egui::Slider::new(&mut live_batch_size, 1..=MAX_BATCH_SIZE).text("Steps per batch (live)"),
        );

        if ui
            .add(
                egui::Slider::new(&mut app.gpu_target_ms, 1.0..=16.0)
                    .text("Target ms/batch")
                    .fixed_decimals(1),
            )
            .changed()
        {
            let target_ms = app.gpu_target_ms;
            if let Some(gpu) = app.sim_thread.as_mut().and_then(SimRunner::as_gpu_mut) {
                gpu.set_target_ms(target_ms);
            }
        }
    } else if ui
        .add(egui::Slider::new(&mut app.gpu_batch_size, 1..=2000).text("Steps per batch"))
        .changed()
    {
        let batch_size = app.gpu_batch_size;
        if let Some(gpu) = app.sim_thread.as_mut().and_then(SimRunner::as_gpu_mut) {
            gpu.set_batch_size(batch_size);
        }
    }
}
