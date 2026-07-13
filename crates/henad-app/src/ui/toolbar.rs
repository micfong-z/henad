use crate::HenadApp;

fn fmt_bytes(bytes: usize) -> String {
    if bytes >= 1 << 30 {
        format!("{:.1} GB", bytes as f64 / (1u64 << 30) as f64)
    } else if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / (1u64 << 20) as f64)
    } else if bytes >= 1 << 10 {
        format!("{:.1} KB", bytes as f64 / (1u64 << 10) as f64)
    } else {
        format!("{bytes} B")
    }
}

fn padded_num_label(ui: &mut egui::Ui, prefix: &str, value: f64, digits: usize) {
    let s = format!("{value:0>digits$.0}");
    let first_sig = s.find(|c: char| c != '0').unwrap_or(s.len() - 1);
    let (zeros, sig) = s.split_at(first_sig);

    let normal = ui.visuals().text_color();
    let faded = egui::Color32::from_gray(80);

    let font = egui::TextFormat {
        font_id: egui::TextStyle::Body.resolve(ui.style()),
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(
        prefix,
        0.0,
        egui::TextFormat {
            color: normal,
            ..font.clone()
        },
    );
    if !zeros.is_empty() {
        job.append(
            zeros,
            0.0,
            egui::TextFormat {
                color: faded,
                ..font.clone()
            },
        );
    }
    job.append(
        sig,
        0.0,
        egui::TextFormat {
            color: normal,
            ..font
        },
    );
    ui.label(job);
}

pub fn toolbar_panel(ctx: &egui::Context, app: &mut HenadApp) {
    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            if let Some(snap) = &app.snapshot {
                ui.label(format!("Tick: {}", snap.tick));
                padded_num_label(ui, "TPS: ", snap.actual_tps, 3);
                ui.label(format!("Pop: {}", snap.population));
                let sim_bytes = snap.heap_bytes + app.pixel_buf.len();
                ui.label(format!("Sim mem: {}", fmt_bytes(sim_bytes)));
            } else {
                ui.label("No simulation loaded");
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::widgets::global_theme_preference_buttons(ui);
                let dt = ctx.input(|i| i.stable_dt);
                if dt > 0.0 {
                    padded_num_label(ui, "FPS: ", (1.0 / dt).into(), 3);
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.separator();
                    let t = &app.timings;
                    let engine = app.snapshot.as_ref().map_or(0.0, |s| s.engine_ms);
                    ui.label(format!(
                        "Engine: {engine:.1}ms  Render: {:.1}ms  UI: {:.1}ms",
                        t.render_ms, t.ui_ms,
                    ));
                }
            });
        });
    });
}
