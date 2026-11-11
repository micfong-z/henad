//! Functions panel of the graph editor: the file's functions with their issue counts and origins, the forms that add,
//! rename and remove a function and edit its ports, and Import function.
//!
//! A function's *ports* are its inputs and outputs. Each call writes the inputs and reads the outputs, and the
//! function's body reads an input through a `fn.input` node and sets an output through a `fn.output` node.

use egui::{Button, ComboBox, Id, RichText, TextEdit, Ui};
use henad_graph::document::{Function, FunctionPort};
use henad_graph::edit::{EditCommand, PortSide, Scope};

use super::passes::{FIELD_WIDTH, commit_field, count_badge};
use super::{GraphEdit, GraphEditor, GraphRequest};
use crate::icons::material_design_icons::{MDI_DELETE_OUTLINE, MDI_FILE_IMPORT_OUTLINE, MDI_PLUS};
use crate::ui::mcs;
use crate::ui::sweep::layout::{add_button, icon_button};

/// Draws the functions panel.
pub fn functions_ui(ui: &mut Ui, graph: &GraphEditor, requests: &mut Vec<GraphRequest>) {
    ui.strong("Functions");
    let document = graph.document();
    if document.functions.is_empty() {
        ui.weak("No functions. Select nodes and press Function, or add one below.");
    }
    for function in &document.functions {
        function_row(ui, graph, function, requests);
    }
    if let Scope::Function(id) = graph.scope()
        && let Some(function) = document.functions.iter().find(|function| function.id == *id)
    {
        ui.add_space(4.0);
        open_function_ui(ui, graph, function, requests);
    }
    ui.add_space(4.0);
    add_function_ui(ui, requests);
    ui.add_space(4.0);
    import_ui(ui, graph, requests);
}

/// Draws the row of `function`: its name, which opens it, its issue counts and the file it was imported from.
fn function_row(ui: &mut Ui, graph: &GraphEditor, function: &Function, requests: &mut Vec<GraphRequest>) {
    let scope = Scope::Function(function.id.clone());
    ui.horizontal(|ui| {
        let open = *graph.scope() == scope;
        if ui
            .selectable_label(open, &function.id)
            .on_hover_text("Open function on canvas")
            .clicked()
        {
            requests.push(GraphRequest::Open(scope.clone()));
        }
        let (errors, warnings) = graph.issue_counts(&scope);
        count_badge(ui, errors, mcs::RED_900, mcs::RED_200, "Errors in this function");
        count_badge(
            ui,
            warnings,
            mcs::ORANGE_900,
            mcs::ORANGE_200,
            "Warnings in this function",
        );
        if let Some(origin) = graph.editor_region().function_origin(&function.id) {
            ui.weak(format!("from {}", origin.name))
                .on_hover_text(format!("Imported from {}, hash {}", origin.name, origin.hash));
        }
    });
}

/// Draws the forms of the open function: rename, its inputs and outputs, and remove.
fn open_function_ui(ui: &mut Ui, graph: &GraphEditor, function: &Function, requests: &mut Vec<GraphRequest>) {
    let form = Id::new("henad_graph_function").with(&function.id);
    ui.horizontal(|ui| {
        ui.label("Name");
        if let Some(to) = commit_field(ui, form.with("name"), &function.id, "function id") {
            requests.push(GraphRequest::Edit(GraphEdit::one(EditCommand::RenameFunction {
                from: function.id.clone(),
                to,
            })));
        }
    });
    for (side, heading, ports) in [
        (PortSide::Input, "Inputs", &function.inputs),
        (PortSide::Output, "Outputs", &function.outputs),
    ] {
        ui.label(RichText::new(heading).small().weak());
        for port in ports {
            port_row(ui, graph, function, side, port, requests);
        }
        let (name, tooltip) = match side {
            PortSide::Input => ("Add input", "Add input and node that reads it"),
            _ => ("Add output", "Add output and node that sets it"),
        };
        let add = add_button(ui, true, Button::new(format!("{MDI_PLUS} {name}")).small(), name).on_hover_text(tooltip);
        if add.clicked() {
            requests.push(GraphRequest::AddFunctionPort {
                function: function.id.clone(),
                side,
            });
        }
    }
    let remove = add_button(
        ui,
        true,
        Button::new(format!("{MDI_DELETE_OUTLINE} Remove function")).small(),
        "Remove function",
    )
    .on_hover_text("Remove function and its nodes");
    if remove.clicked() {
        requests.push(GraphRequest::Edit(GraphEdit::one(EditCommand::RemoveFunction(
            function.id.clone(),
        ))));
    }
}

/// Draws the row of one port of `function`: its name, its type and Remove.
fn port_row(
    ui: &mut Ui,
    graph: &GraphEditor,
    function: &Function,
    side: PortSide,
    port: &FunctionPort,
    requests: &mut Vec<GraphRequest>,
) {
    let row = Id::new("henad_graph_function_port").with((&function.id, side == PortSide::Input, &port.port));
    ui.horizontal(|ui| {
        if let Some(to) = commit_field(ui, row.with("name"), &port.port, "port name") {
            requests.push(GraphRequest::Edit(GraphEdit::one(EditCommand::RenameFunctionPort {
                function: function.id.clone(),
                side,
                from: port.port.clone(),
                to,
            })));
        }
        ComboBox::from_id_salt(row.with("type"))
            .width(60.0)
            .selected_text(port.value_type.to_string())
            .show_ui(ui, |ui| {
                for value_type in super::canvas::value_types(graph.document()) {
                    let picked = ui
                        .selectable_label(value_type == port.value_type, value_type.to_string())
                        .clicked();
                    if picked && value_type != port.value_type {
                        requests.push(GraphRequest::Edit(GraphEdit::one(EditCommand::SetFunctionPortType {
                            function: function.id.clone(),
                            side,
                            port: port.port.clone(),
                            value_type,
                        })));
                    }
                }
            });
        if icon_button(
            ui,
            MDI_DELETE_OUTLINE,
            "Remove port",
            "Remove port, once no node names it",
        )
        .clicked()
        {
            requests.push(GraphRequest::Edit(GraphEdit::one(EditCommand::RemoveFunctionPort {
                function: function.id.clone(),
                side,
                port: port.port.clone(),
            })));
        }
    });
}

/// Draws the form that adds an empty function.
fn add_function_ui(ui: &mut Ui, requests: &mut Vec<GraphRequest>) {
    let form_id = Id::new("henad_graph_add_function");
    let mut id = ui.data(|data| data.get_temp::<String>(form_id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add(
            TextEdit::singleline(&mut id)
                .hint_text("new function id")
                .desired_width(FIELD_WIDTH - 40.0),
        );
        let add = add_button(
            ui,
            !id.is_empty(),
            Button::new(format!("{MDI_PLUS} Add")),
            "Add function",
        )
        .on_hover_text("Add empty function")
        .on_disabled_hover_text("Enter an id for the function.");
        if add.clicked() {
            requests.push(GraphRequest::Edit(GraphEdit::one(EditCommand::AddFunction(id.clone()))));
            requests.push(GraphRequest::Open(Scope::Function(id.clone())));
            id.clear();
        }
    });
    ui.data_mut(|data| data.insert_temp(form_id, id));
}

/// Draws Import function, and the functions of the picked file with a name each takes here.
fn import_ui(ui: &mut Ui, graph: &GraphEditor, requests: &mut Vec<GraphRequest>) {
    let pick = add_button(
        ui,
        true,
        Button::new(format!("{MDI_FILE_IMPORT_OUTLINE} Import function")),
        "Import function",
    )
    .on_hover_text("Copy function from another model file");
    if pick.clicked() {
        requests.push(GraphRequest::PickImport);
    }
    let Some(source) = graph.import_source() else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Functions in {}", source.name)).small().weak());
        if ui.small_button("Close").on_hover_text("Forget this file").clicked() {
            requests.push(GraphRequest::CloseImport);
        }
    });
    for function in &source.functions {
        let field_id = Id::new("henad_graph_import_as").with((&source.name, function));
        let mut name = ui
            .data(|data| data.get_temp::<String>(field_id))
            .unwrap_or_else(|| function.clone());
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut name)
                    .hint_text(function.as_str())
                    .desired_width(FIELD_WIDTH - 40.0),
            )
            .on_hover_text(format!("Name the function '{function}' takes in this model"));
            let import = add_button(ui, !name.is_empty(), Button::new("Import"), "Import")
                .on_hover_text("Copy function and the functions it calls");
            if import.clicked() {
                requests.push(GraphRequest::Import {
                    function: function.clone(),
                    id: (name != *function).then(|| name.clone()),
                });
            }
        });
        ui.data_mut(|data| data.insert_temp(field_id, name));
    }
}
