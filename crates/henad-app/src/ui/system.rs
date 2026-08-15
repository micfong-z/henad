//! Host and GPU information.

use crate::icons::material_design_icons::{MDI_ALERT, MDI_INFORMATION};
use crate::state::AppState;
use crate::ui::{banner, kv_grid};
use henad_compute::runtime_info::{GpuVerdict, RuntimeInfo, classify_adapter};
use henad_core::helpers::fmt_bytes;

fn device_type_name(device_type: wgpu::DeviceType) -> &'static str {
    match device_type {
        wgpu::DeviceType::Other => "Unknown",
        wgpu::DeviceType::IntegratedGpu => "Integrated GPU",
        wgpu::DeviceType::DiscreteGpu => "Discrete GPU",
        wgpu::DeviceType::VirtualGpu => "Virtual GPU",
        wgpu::DeviceType::Cpu => "CPU (software)",
    }
}

fn gpu_warnings(ui: &mut egui::Ui, info: &RuntimeInfo) {
    match classify_adapter(&info.adapter) {
        GpuVerdict::Capable => {}
        GpuVerdict::Uncertain => banner(
            ui,
            MDI_INFORMATION,
            ui.visuals().text_color(),
            "GPU performance uncertain",
            "No discrete GPU was directly detected. This may be a basic integrated GPU, a virtual GPU, or a SoC. \
            If you believe you have a discrete GPU, check that your OS is configured to use it for Henad.",
        ),
        GpuVerdict::Absent => banner(
            ui,
            MDI_ALERT,
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

            ui.label("Timestamp query");
            ui.label(if info.timestamp_query {
                "Supported"
            } else {
                "Unavailable"
            });
            ui.end_row();
        });

        ui.add_space(8.0);
        ui.strong("Device limits");
        ui.add_space(4.0);
        limits_grid(ui, info);
    });
}

fn limits_grid(ui: &mut egui::Ui, info: &RuntimeInfo) {
    let (granted, available) = (&info.granted, &info.available);

    kv_grid(ui, "system_limits_grid").show(ui, |ui| {
        let mut row = |label: &str, granted: String, available: String| {
            ui.label(label);
            ui.horizontal(|ui| {
                ui.style_mut().spacing.item_spacing.x = 0.0;
                let differs = granted != available;
                ui.label(granted);
                if differs {
                    ui.weak(format!(" of {available}"));
                }
            });
            ui.end_row();
        };

        row(
            "Max storage binding",
            fmt_bytes(granted.max_storage_buffer_binding_size),
            fmt_bytes(available.max_storage_buffer_binding_size),
        );
        row(
            "Max buffer size",
            fmt_bytes(granted.max_buffer_size),
            fmt_bytes(available.max_buffer_size),
        );
        row(
            "Max 2D texture",
            granted.max_texture_dimension_2d.to_string(),
            available.max_texture_dimension_2d.to_string(),
        );
        row(
            "Storage buffers / stage",
            granted.max_storage_buffers_per_shader_stage.to_string(),
            available.max_storage_buffers_per_shader_stage.to_string(),
        );
        row(
            "Workgroups / dimension",
            granted.max_compute_workgroups_per_dimension.to_string(),
            available.max_compute_workgroups_per_dimension.to_string(),
        );
        row(
            "Invocations / workgroup",
            granted.max_compute_invocations_per_workgroup.to_string(),
            available.max_compute_invocations_per_workgroup.to_string(),
        );
        row(
            "Device texture cap",
            info.display_cap().to_string(),
            info.display_cap().to_string(),
        );
    });
}
