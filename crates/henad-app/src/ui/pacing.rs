//! How fast the sim is allowed to run, and how often it hands back a snapshot.

use crate::state::AppState;
use henad_compute::cpu::sim_thread::SimCommand;

#[cfg(not(target_arch = "wasm32"))]
use crate::sim_runner::SimRunner;

pub fn pacing_ui(ui: &mut egui::Ui, app: &mut AppState) {
    // A GPU model paces itself with the batch-size controller below and has no notion of a TPS cap
    // or a tick-based snapshot cadence, so the CPU pacing controls are swapped out for the GPU ones
    // rather than shown alongside them (they would be inert).
    if app.is_gpu() {
        #[cfg(not(target_arch = "wasm32"))]
        gpu_batching_controls(ui, app);
    } else {
        cpu_pacing_controls(ui, app);
    }
}

fn cpu_pacing_controls(ui: &mut egui::Ui, app: &mut AppState) {
    let uncapped_changed = ui.checkbox(&mut app.uncapped, "Unlimited TPS").changed();

    if !app.uncapped {
        let tps_changed = ui
            .add(
                egui::Slider::new(&mut app.target_tps, 1.0..=1000.0)
                    .logarithmic(true)
                    .text("Target TPS"),
            )
            .changed();
        if tps_changed && let Some(thread) = &mut app.sim_thread {
            thread.send(SimCommand::SetTargetTps(app.target_tps));
        }
    }

    if uncapped_changed && let Some(thread) = &mut app.sim_thread {
        thread.send(SimCommand::SetUncapped(app.uncapped));
    }

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

/// Batching controls, shown in place of the CPU pacing controls for a GPU model.
#[cfg(not(target_arch = "wasm32"))]
fn gpu_batching_controls(ui: &mut egui::Ui, app: &mut AppState) {
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
