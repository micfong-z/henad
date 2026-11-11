//! The Pacing panel: a model's speed limit, a CPU model's snapshot cadence, a network's layout, and a GPU model's
//! batching.

use crate::sim_runner::SimRunner;
use crate::state::AppState;
use henad_compute::cpu::sim_thread::SimCommand;
use henad_compute::runner::MAX_VIEW_BUDGET_MS;

const LAYOUT_BUDGET_MAX_MS: f32 = 50.0;

pub fn pacing_ui(ui: &mut egui::Ui, app: &mut AppState) {
    // A GPU model ignores the snapshot cadence. Its batching controls take the place of the cadence.
    tps_controls(ui, app);
    if app.is_gpu() {
        gpu_batching_controls(ui, app);
    } else {
        cpu_pacing_controls(ui, app);
    }
}

/// Draws the speed limit, which both runners keep to.
fn tps_controls(ui: &mut egui::Ui, app: &mut AppState) {
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
            app.send_command(SimCommand::SetTargetTps(app.target_tps));
        }
    }

    if uncapped_changed {
        app.send_command(SimCommand::SetUncapped(app.uncapped));
    }
}

fn cpu_pacing_controls(ui: &mut egui::Ui, app: &mut AppState) {
    if ui
        .add(
            egui::Slider::new(&mut app.ticks_per_snapshot, 1..=1000)
                .logarithmic(true)
                .text("Ticks/snapshot"),
        )
        .changed()
    {
        app.send_command(SimCommand::SetTicksPerSnapshot(app.ticks_per_snapshot));
    }

    let network = app.loaded_entry().is_some_and(|entry| entry.topology_hint().edges);
    if network {
        layout_controls(ui, app);
    }
}

fn layout_controls(ui: &mut egui::Ui, app: &mut AppState) {
    ui.separator();
    let mut changed = ui
        .checkbox(&mut app.layout_on, "Layout")
        .on_hover_text("Arrange network nodes with a spring layout when running")
        .changed();
    changed |= ui
        .add_enabled(
            app.layout_on,
            egui::Checkbox::new(&mut app.layout_while_paused, "Layout while paused"),
        )
        .on_hover_text("Keep arranging nodes while paused")
        .changed();
    changed |= ui
        .add_enabled(
            app.layout_on,
            egui::Slider::new(
                &mut app.layout_budget_ms,
                1.0..=LAYOUT_BUDGET_MAX_MS.min(MAX_VIEW_BUDGET_MS),
            )
            .text("Layout budget")
            .suffix(" ms"),
        )
        .on_hover_text("Maximum time for layout per snapshot")
        .changed();
    if changed {
        app.send_command(app.layout_command());
    }
}

/// Draws the batching controls a GPU model shows instead of the CPU pacing controls.
fn gpu_batching_controls(ui: &mut egui::Ui, app: &mut AppState) {
    use henad_compute::gpu::timing::MAX_BATCH_SIZE;

    let stats = app
        .sim_thread
        .as_ref()
        .and_then(SimRunner::gpu_stats)
        .unwrap_or_default();

    ui.label(match stats.gpu_us_per_step {
        Some(us) => format!("GPU time/step: {us:.2} \u{b5}s"),
        None => "GPU time/step: N/A".to_owned(),
    });

    let mut changed = ui
        .checkbox(&mut app.gpu_batch.adaptive, "Adaptive batching")
        .on_hover_text(
            "Automatically pick steps-per-batch to keep each GPU submission under the target \
             time below, instead of a fixed batch size, so large batches don't block egui's own \
             rendering on the shared queue.",
        )
        .changed();

    if app.gpu_batch.adaptive {
        // The disabled slider shows the controller's batch size, read from the stats every frame to track the GPU
        // cost.
        let mut live_batch_size = stats.batch_size;
        ui.add_enabled(
            false,
            egui::Slider::new(&mut live_batch_size, 1..=MAX_BATCH_SIZE).text("Steps per batch (live)"),
        );

        changed |= ui
            .add(
                egui::Slider::new(&mut app.gpu_batch.target_ms, 1.0..=16.0)
                    .text("Target ms/batch")
                    .fixed_decimals(1),
            )
            .changed();
    } else {
        changed |= ui
            .add(egui::Slider::new(&mut app.gpu_batch.batch_size, 1..=2000).text("Steps per batch"))
            .changed();
    }

    if changed && let Some(runner) = &mut app.sim_thread {
        runner.set_gpu_batch(app.gpu_batch);
    }
}
