//! Generated WGSL view: the read-only text of each shader that runs the open pass, as the compiler wrote it.
//!
//! A tick pass has a shader of its own, the view passes share the display shader, the stats passes the reduce shader,
//! and each action pass has one per action. An init pass runs on the host when the model is built and has no shader.
//! The text is the compiler's, with its `#import` lines, before composition with the shared modules.

use egui::{Label, RichText, ScrollArea, TextWrapMode};
use henad_graph::document::{PassEnabled, PassStage};

use super::GraphEditor;

/// Text shown for an init pass, which no shader runs.
pub const HOST_PASS: &str = "Init passes run on the host when the model is built. They have no WGSL.";

/// Text shown for a pass that is switched off.
pub const DISABLED_PASS: &str = "Pass is switched off, and no shader runs it.";

/// Text shown while the model does not compile.
pub const NOT_COMPILED: &str = "Model does not compile. Fix the problems below to see its WGSL.";

/// Draws the generated WGSL of the open pass.
pub fn wgsl_ui(ui: &mut egui::Ui, graph: &mut GraphEditor) {
    ui.strong("Generated WGSL");
    let Some(pass) = graph.open_pass() else {
        ui.weak("Open a pass to see its WGSL.");
        return;
    };
    let id = pass.id.clone();
    if pass.stage == PassStage::Init {
        ui.weak(HOST_PASS);
        return;
    }
    if pass.enabled == PassEnabled::Literal(false) {
        ui.weak(DISABLED_PASS);
        return;
    }
    let Ok(program) = graph.program() else {
        ui.weak(NOT_COMPILED);
        return;
    };
    let shaders: Vec<_> = program
        .shaders()
        .iter()
        .filter(|shader| shader.passes().contains(&id))
        .collect();
    if shaders.is_empty() {
        ui.weak(DISABLED_PASS);
        return;
    }
    ScrollArea::both()
        .id_salt("henad_graph_wgsl_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            for shader in shaders {
                let others = shader.passes().len() - 1;
                let heading = match others {
                    0 => format!("Shader {}", shader.label()),
                    1 => format!("Shader {}, with 1 other pass", shader.label()),
                    count => format!("Shader {}, with {count} other passes", shader.label()),
                };
                ui.label(RichText::new(heading).weak());
                ui.add(
                    Label::new(RichText::new(shader.text()).monospace())
                        .wrap_mode(TextWrapMode::Extend)
                        .selectable(true),
                );
            }
        });
}
