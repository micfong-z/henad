//! Modal shown when the engine caught something that stopped the simulation.

use egui::{Context, Id, Modal, RichText, ScrollArea};

use crate::icons::material_design_icons::{MDI_ALERT_CIRCLE_OUTLINE, MDI_CONTENT_COPY};
use crate::state::AppState;
use henad_compute::fault::{BUILDING, FaultKind};

/// Past this a long validation message scrolls instead of pushing the buttons off.
const MAX_MESSAGE_HEIGHT: f32 = 220.0;

pub fn fault_modal(ctx: &Context, app: &mut AppState) {
    let Some(fault) = &app.fault else {
        return;
    };

    let title = if fault.during == BUILDING {
        "Model build failed"
    } else {
        "Simulation aborted"
    };
    let subject = app
        .registry
        .get(app.selected_model)
        .map_or_else(|| "Model".to_owned(), |entry| entry.name.clone());
    let lead = match &fault.kind {
        FaultKind::Device(_) => format!("GPU reported an error while {} for {subject}.", fault.during),
        FaultKind::Panic { location: Some(at), .. } => {
            format!("{subject} panicked while {}, at {at}.", fault.during)
        }
        FaultKind::Panic { location: None, .. } => format!("{subject} panicked while {}.", fault.during),
        FaultKind::Refused(_) => format!("{subject} cannot run on this device."),
    };
    let detail = match &fault.kind {
        FaultKind::Device(error) => error.to_string(),
        FaultKind::Panic { message, .. } | FaultKind::Refused(message) => message.clone(),
    };

    let mut dismissed = false;
    let response = Modal::new(Id::new("henad_fault_modal")).show(ctx, |ui| {
        ui.set_max_width(560.0);
        ui.horizontal(|ui| {
            ui.colored_label(
                ui.visuals().error_fg_color,
                RichText::new(MDI_ALERT_CIRCLE_OUTLINE).heading(),
            );
            ui.heading(title);
        });
        ui.add_space(4.0);
        ui.label(lead);
        ui.add_space(8.0);

        ScrollArea::vertical().max_height(MAX_MESSAGE_HEIGHT).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(&detail).monospace()).selectable(true));
        });

        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .button(format!("{MDI_CONTENT_COPY} Copy"))
                .on_hover_text("Copy message to clipboard")
                .clicked()
            {
                ui.ctx().copy_text(detail.clone());
            }
            // Nothing is left running. This only clears the message. A captured error leaves the
            // device usable and Build stays available.
            if ui.button("Dismiss").clicked() {
                dismissed = true;
            }
        });
    });

    if dismissed || response.should_close() {
        app.fault = None;
    }
}
