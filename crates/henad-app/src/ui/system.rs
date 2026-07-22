//! Host and GPU information.

use crate::icons::material_design_icons::{MDI_ALERT_OUTLINE, MDI_INFORMATION_OUTLINE};
use crate::state::AppState;
use crate::ui::{fmt_bytes, kv_grid};
use henad_compute::runtime_info::{GpuVerdict, RuntimeInfo, classify_adapter};

fn device_type_name(device_type: wgpu::DeviceType) -> &'static str {
    match device_type {
        wgpu::DeviceType::Other => "Unknown",
        wgpu::DeviceType::IntegratedGpu => "Integrated GPU",
        wgpu::DeviceType::DiscreteGpu => "Discrete GPU",
        wgpu::DeviceType::VirtualGpu => "Virtual GPU",
        wgpu::DeviceType::Cpu => "CPU (software)",
    }
}

fn info_banner(ui: &mut egui::Ui, color: egui::Color32, title: &str, detail: &str) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(color, MDI_INFORMATION_OUTLINE);
            ui.colored_label(color, title);
        });
        ui.label(detail);
    });
}

fn warning_banner(ui: &mut egui::Ui, color: egui::Color32, title: &str, detail: &str) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(color, MDI_ALERT_OUTLINE);
            ui.colored_label(color, title);
        });
        ui.label(detail);
    });
}

fn gpu_warnings(ui: &mut egui::Ui, info: &RuntimeInfo) {
    match classify_adapter(&info.adapter) {
        GpuVerdict::Capable => {}
        GpuVerdict::Uncertain => info_banner(
            ui,
            ui.visuals().text_color(),
            "GPU performance uncertain",
            "No discrete GPU was directly detected. This may be a basic integrated GPU, a virtual GPU, or a SoC. \
            If you believe you have a discrete GPU, check that your OS is configured to use it for Henad.",
        ),
        GpuVerdict::Absent => warning_banner(
            ui,
            ui.visuals().error_fg_color,
            "No GPU detected",
            "Rendering is going through a software rasteriser. GPU-backed models will likely be \
             extremely slow.",
        ),
    }
}

pub fn system_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let info = &app.runtime;

    egui::ScrollArea::vertical().show(ui, |ui| {
        gpu_warnings(ui, info);

        ui.add_space(4.0);
        ui.strong("Host");
        kv_grid(ui, "system_host_grid").show(ui, |ui| {
            ui.label("Platform");
            ui.label(format!("{} ({})", info.host.os, info.host.arch));
            ui.end_row();

            ui.label("Logical CPUs");
            ui.label(
                info.host
                    .logical_cpus
                    .map_or_else(|| "Unknown".to_owned(), |n| n.to_string()),
            );
            ui.end_row();

            ui.label("Worker threads");
            ui.label(
                info.host
                    .worker_threads
                    .map_or_else(|| "None (single-threaded)".to_owned(), |n| n.to_string()),
            );
            ui.end_row();
        });

        ui.add_space(8.0);
        ui.strong("Graphics");
        kv_grid(ui, "system_gpu_grid").show(ui, |ui| {
            ui.label("Adapter");
            ui.label(info.adapter.name.as_str());
            ui.end_row();

            ui.label("Type");
            ui.label(device_type_name(info.adapter.device_type));
            ui.end_row();

            ui.label("Backend");
            ui.label(info.adapter.backend.to_string());
            ui.end_row();

            if !info.adapter.driver_info.is_empty() {
                ui.label("Driver");
                ui.label(info.adapter.driver_info.as_str());
                ui.end_row();
            }

            // The binding cap, not VRAM, is what bounds a GPU model's grid.
            ui.label("Max storage binding");
            ui.label(fmt_bytes(info.max_storage_binding_bytes))
                .on_hover_text(format!("{} u32 cells per binding", info.max_storage_binding_bytes / 4));
            ui.end_row();

            ui.label("Timestamp query");
            ui.label(if info.timestamp_query {
                "Supported"
            } else {
                "Unavailable"
            });
            ui.end_row();
        });
    });
}
