//! The checks a grid rule passes before it runs: its declarations, its text, and the naga IR of its functions.
//!
//! A rule's step runs once per cell per tick and writes only its own cell. The checks hold it to that. Its functions
//! hold no loop, and one call of `step_cell` runs at most [`MAX_STEP_COST`] expressions and statements, so a step's
//! work is bounded. They read no buffer of the engine but `params`, write no memory but their own locals and the rule's
//! `private` variables, and use no atomics, barriers or textures. Their variables stay within WGSL's limits of
//! [`MAX_FUNCTION_BYTES`] per function and [`MAX_PRIVATE_BYTES`] for every `private` variable of the shader.

use std::collections::{HashMap, HashSet};

use henad_core::diagnostic::CodeDiagnostic;
use henad_core::helpers::check_choice_options;
use henad_core::params::{ParamDescriptor, ParamFormat, ParamKind};
use naga::{AddressSpace, Block, Expression, Function, Handle, Module, Span, Statement, TypeInner};

use super::template::{FieldType, PRIVATE_PREFIX, TEMPLATE_NAMES, constant_name};
use super::{GridRule, MAX_GRID_SIDE, MAX_PARAMS, MAX_STATES, MAX_STEP_BYTES, RuleDeclaration};
use crate::gpu::compose::{
    Composition, Located, code_lines, find_wgsl_only_line_break, undecorate, wgsl_only_line_break_message, words,
};

/// Most bytes the local variables of one function hold, WGSL's limit on the function address space.
pub(crate) const MAX_FUNCTION_BYTES: u64 = 8192;

/// Most bytes the `private` variables of a shader hold, the template's included, WGSL's limit on the private address
/// space.
pub(crate) const MAX_PRIVATE_BYTES: u64 = 8192;

/// Most expressions and statements one call of `step_cell` runs, counting the body of each function it calls once per
/// call.
pub(crate) const MAX_STEP_COST: u64 = 1 << 16;

/// Grid parameter ids the engine puts in front of a rule's own parameter ids.
const GRID_PARAM_IDS: &[&str] = &["grid_width", "grid_height"];

/// Parameter ids the template keeps for itself besides WGSL's reserved words: the fields it fills in `Params`, and
/// `tick`.
const RESERVED_PARAM_IDS: &[&str] = &["width", "height", "seed", "tick"];

/// Signature the template calls the rule's step with, as a message names it.
pub(crate) const STEP_SIGNATURE: &str = "fn step_cell(cell: u32, rng: ptr<function, u32>) -> u32";

/// Engine buffer the rule's text reads, the uniform of its parameters.
const PARAMS_BUFFER: &str = "params";

/// State constants, parameter fields and initial shares that the declarations of a rule fix for its template and its
/// hooks.
#[derive(Debug)]
pub(crate) struct Declared {
    /// Constant name of each state, in state order.
    pub(crate) constants: Vec<String>,
    /// Name and type of each field of `Params` after [`PARAMS_HEAD`](super::template::PARAMS_HEAD), one per parameter
    /// of the rule.
    pub(crate) fields: Vec<(String, FieldType)>,
    /// For each state with an initial share, the state and the position of its parameter among the rule's own
    /// parameters, in state order.
    pub(crate) shares: Vec<(u32, usize)>,
    /// State a cell takes when no share claims it: the first state without an initial share.
    pub(crate) rest: u32,
}

/// A problem in a rule's declarations, with the declaration it is about when there is one.
pub(crate) type DeclarationProblem = (Option<RuleDeclaration>, CodeDiagnostic);

/// Checks the declarations of `rule`: its id, name, description, size, states and parameters. Returns the problems, or
/// the [`Declared`] values when the declarations have no problem.
pub(crate) fn check_declarations(rule: &GridRule) -> Result<Declared, Vec<DeclarationProblem>> {
    let mut problems = Vec::new();
    check_names(rule, &mut problems);
    for (side, value) in [("width", rule.width), ("height", rule.height)] {
        if value == 0 || value > MAX_GRID_SIDE {
            problems.push((
                Some(RuleDeclaration::Grid),
                CodeDiagnostic::error(format!(
                    "the grid {side} is {value}, and a rule's grid side is 1 to {MAX_GRID_SIDE}"
                )),
            ));
        }
    }

    let fields = check_params(&rule.params, &mut problems);

    if rule.states.is_empty() {
        problems.push((None, CodeDiagnostic::error("a rule declares at least one state")));
    }
    if rule.states.len() > MAX_STATES {
        problems.push((
            None,
            CodeDiagnostic::error(format!(
                "the rule declares {} states, and a rule takes at most {MAX_STATES}",
                rule.states.len()
            )),
        ));
    }
    let mut constants: Vec<String> = Vec::with_capacity(rule.states.len());
    let mut shares = Vec::new();
    let mut rest = None;
    for (index, state) in rule.states.iter().enumerate() {
        let mut problem = |message: String| {
            problems.push((Some(RuleDeclaration::State(index)), CodeDiagnostic::error(message)));
        };
        if let Some(message) = control_message(&format!("the label of state {index}"), &state.label) {
            problem(message);
        }
        let (constant, refusal) = state_constant(&state.label);
        if let Some(message) = refusal {
            problem(message);
        }
        if !constant.is_empty()
            && let Some(other) = constants.iter().position(|earlier| *earlier == constant)
        {
            problem(format!(
                "states '{}' and '{}' both give the constant `{constant}`",
                rule.states[other].label, state.label
            ));
        }
        constants.push(constant);
        let state_index = u32::try_from(index).unwrap_or(u32::MAX);
        match &state.initial {
            Some(id) => match rule.params.iter().position(|param| param.id() == id) {
                Some(position) if matches!(rule.params[position].kind(), ParamKind::F32 { .. }) => {
                    shares.push((state_index, position));
                }
                Some(_) => problem(format!(
                    "state '{}' takes its initial share from '{id}', which is not an f32 parameter",
                    state.label
                )),
                None => problem(format!(
                    "state '{}' takes its initial share from '{id}', and the rule declares no such parameter",
                    state.label
                )),
            },
            None => {
                rest.get_or_insert(state_index);
            }
        }
    }
    if !rule.states.is_empty() && rest.is_none() {
        problems.push((
            None,
            CodeDiagnostic::error(
                "every state takes an initial share. Leave one state without `initial`, and it takes the cells no \
                 share claims",
            ),
        ));
    }
    if problems.is_empty() {
        Ok(Declared {
            constants,
            fields,
            shares,
            rest: rest.unwrap_or(0),
        })
    } else {
        Err(problems)
    }
}

/// Checks the rule's id against the id grammar, and its name and description for control characters.
fn check_names(rule: &GridRule, problems: &mut Vec<DeclarationProblem>) {
    if let Err(reason) = crate::entry::check_id_grammar(&rule.id) {
        problems.push((
            Some(RuleDeclaration::Id),
            CodeDiagnostic::error(format!(
                "rule id '{}' {reason} (an id is a lowercase ASCII letter, then lowercase letters, digits and \
                 underscores)",
                rule.id
            )),
        ));
    }
    for (holder, text) in [
        ("the rule's name", &rule.name),
        ("the rule's description", &rule.description),
    ] {
        if let Some(message) = control_message(holder, text) {
            problems.push((None, CodeDiagnostic::error(message)));
        }
    }
}

/// Returns the message that `holder` holds a control character, when `text` holds one.
fn control_message(holder: &str, text: &str) -> Option<String> {
    let character = text.chars().find(|character| character.is_control())?;
    Some(format!(
        "{holder} holds the control character U+{:04X}",
        u32::from(character)
    ))
}

/// Returns the constant name of the state labelled `label`, empty when the label gives none, and the reason the name
/// cannot serve, if any.
fn state_constant(label: &str) -> (String, Option<String>) {
    match constant_name(label) {
        Some(name) if name.starts_with(|c: char| c.is_ascii_digit()) => {
            let reason = format!("state '{label}' gives the constant `{name}`, which starts with a digit");
            (name, Some(reason))
        }
        Some(name) if is_reserved_name(&name) => {
            let reason =
                format!("state '{label}' gives the constant `{name}`, which the rule template already declares");
            (name, Some(reason))
        }
        Some(name) if naga::keywords::wgsl::RESERVED.contains(&name.as_str()) => {
            let reason = format!("state '{label}' gives the constant `{name}`, which is a reserved word of WGSL");
            (name, Some(reason))
        }
        Some(name) => (name, None),
        None => (
            String::new(),
            Some(format!(
                "state '{label}' gives no constant name. A label holds at least one ASCII letter or digit"
            )),
        ),
    }
}

/// Checks the rule's parameters and returns the field of each.
fn check_params(params: &[ParamDescriptor], problems: &mut Vec<DeclarationProblem>) -> Vec<(String, FieldType)> {
    if params.len() > MAX_PARAMS {
        problems.push((
            None,
            CodeDiagnostic::error(format!(
                "the rule declares {} parameters, and a rule takes at most {MAX_PARAMS}",
                params.len()
            )),
        ));
    }
    let mut seen = HashSet::new();
    let mut fields = Vec::with_capacity(params.len());
    for (index, param) in params.iter().enumerate() {
        let id = param.id();
        let mut problem = |message: String| {
            problems.push((Some(RuleDeclaration::Param(index)), CodeDiagnostic::error(message)));
        };
        if let Err(reason) = crate::entry::check_id_grammar(id) {
            problem(format!("parameter id '{id}' {reason}"));
        } else if RESERVED_PARAM_IDS.contains(&id) || GRID_PARAM_IDS.contains(&id) || id.starts_with(PRIVATE_PREFIX) {
            problem(format!(
                "parameter id '{id}' is reserved. The engine fills `params.width`, `params.height` and `params.seed`, \
                 and declares `grid_width` and `grid_height`"
            ));
        } else if naga::keywords::wgsl::RESERVED.contains(&id) {
            problem(format!("parameter id '{id}' is a WGSL keyword"));
        }
        if !seen.insert(id) {
            problem(format!("parameter id '{id}' is declared twice"));
        }
        if let Some(message) = control_message(&format!("the label of parameter '{id}'"), param.label()) {
            problem(message);
        }
        if let ParamKind::Choice { options, .. } = param.kind()
            && let Some(message) = options
                .iter()
                .find_map(|option| control_message(&format!("an option of parameter '{id}'"), option))
        {
            problem(message);
        }
        if let Err(reason) = check_kind(param.kind()) {
            problem(format!("parameter '{id}' {reason}"));
        }
        if param.format() == ParamFormat::Percent && !matches!(param.kind(), ParamKind::F32 { .. }) {
            problem(format!(
                "parameter '{id}' shows as a percentage, and only an f32 parameter holds a fraction"
            ));
        }
        let field = match param.kind() {
            ParamKind::F32 { .. } => FieldType::F32,
            _ => FieldType::U32,
        };
        fields.push((id.to_owned(), field));
    }
    fields
}

/// Checks the bounds, step, default and options of a parameter's kind, and returns the problem as a clause after the
/// parameter's name.
fn check_kind(kind: &ParamKind) -> Result<(), String> {
    match kind {
        ParamKind::F32 {
            min,
            max,
            default,
            step,
        } => {
            if ![*min, *max, *default].iter().all(|value| value.is_finite()) {
                Err("holds a bound or default that is not finite".to_owned())
            } else if !(min <= default && default <= max) {
                Err(format!(
                    "has the default {default:?}, outside its bounds {min:?} to {max:?}"
                ))
            } else if step.is_some_and(|step| !(step.is_finite() && step > 0.0)) {
                Err("has a step that is not a positive finite number".to_owned())
            } else {
                Ok(())
            }
        }
        ParamKind::U32 { min, max, default } => {
            if min <= default && default <= max {
                Ok(())
            } else {
                Err(format!("has the default {default}, outside its bounds {min} to {max}"))
            }
        }
        ParamKind::Bool { .. } => Ok(()),
        ParamKind::Choice { options, default } => {
            let names: Vec<&str> = options.iter().collect();
            check_choice_options(&names)?;
            if *default < names.len() {
                Ok(())
            } else {
                Err(format!(
                    "has {} options, and no option at its default index {default}",
                    names.len()
                ))
            }
        }
    }
}

/// Returns whether `name` belongs to the rule template.
fn is_reserved_name(name: &str) -> bool {
    TEMPLATE_NAMES.contains(&name)
        || name
            .get(..PRIVATE_PREFIX.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(PRIVATE_PREFIX))
}

/// Checks the rule's text before it is composed: its size, its line breaks, the names it declares, its attributes and
/// its `step_cell`.
///
/// `constants` are the state constants, which the text cannot declare again. Each problem is placed in the text.
pub(crate) fn check_text(step: &str, constants: &[String]) -> Vec<CodeDiagnostic> {
    if step.len() > MAX_STEP_BYTES {
        return vec![CodeDiagnostic::error(format!(
            "the rule's text holds {} bytes, and a rule's text holds at most {MAX_STEP_BYTES}",
            step.len()
        ))];
    }
    // naga ends a `//` comment at a line break such as U+2028, and the checks below would read the rest of the line
    // as comment.
    if let Some((start, character)) = find_wgsl_only_line_break(step) {
        return vec![
            CodeDiagnostic::error(wgsl_only_line_break_message(character))
                .at(step, start..start + character.len_utf8()),
        ];
    }
    let code = blank_comments(step);
    let bytes = code.as_bytes();
    let mut problems = Vec::new();
    let place = |diagnostic: CodeDiagnostic, start: usize, len: usize| diagnostic.at(step, start..start + len);
    let mut defines_step = false;
    // One binding problem per line, for a declaration that carries both `@group` and `@binding`.
    let mut binding_line_end = 0;
    // Depth of braces at the start of the current word, with the bytes before it counted up to `scanned`.
    let mut depth = 0usize;
    let mut scanned = 0;
    let found: Vec<(usize, &str)> = words(&code).collect();
    for (position, &(offset, word)) in found.iter().enumerate() {
        for &byte in &bytes[scanned..offset] {
            match byte {
                b'{' => depth += 1,
                b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        scanned = offset;
        // naga reads an attribute's name after an `@` and any blank or comment.
        let at = code[..offset].trim_end().strip_suffix('@').map(str::len);
        if let Some(at) = at
            && matches!(word, "compute" | "vertex" | "fragment")
        {
            problems.push(place(
                CodeDiagnostic::error(format!(
                    "a grid rule declares no entry point. The engine's `main` calls `step_cell`, and `@{word}` \
                     has no use"
                )),
                at,
                offset + word.len() - at,
            ));
            continue;
        }
        if word == "group" || word == "binding" {
            if let Some(at) = at
                && offset >= binding_line_end
            {
                binding_line_end = code[offset..].find('\n').map_or(code.len(), |end| offset + end);
                problems.push(place(
                    CodeDiagnostic::error(
                        "a grid rule declares no bindings. The engine binds the cells and `params` for the rule",
                    ),
                    at,
                    offset + word.len() - at,
                ));
            }
            continue;
        }
        if !matches!(word, "fn" | "const" | "let" | "var" | "struct" | "alias") {
            continue;
        }
        let Some((name_offset, name)) = declared_name(&code, offset + word.len(), &found[position + 1..]) else {
            continue;
        };
        if word == "fn" && name == "step_cell" {
            defines_step = true;
        }
        if is_reserved_name(name) || constants.iter().any(|constant| constant == name) {
            problems.push(place(
                CodeDiagnostic::error(format!(
                    "`{name}` is a name of the rule template, and cannot be declared again"
                )),
                name_offset,
                name.len(),
            ));
        } else if depth == 0 && naga::keywords::wgsl::BUILTIN_IDENTIFIERS.contains(&name) {
            // A module-scope item of the same name would take the place of the built-in in the template's own lines.
            problems.push(place(
                CodeDiagnostic::error(builtin_name_message(name)),
                name_offset,
                name.len(),
            ));
        }
    }
    if !defines_step && problems.is_empty() {
        problems.push(
            CodeDiagnostic::error("the rule defines no `step_cell`").with_note(format!("define `{STEP_SIGNATURE}`")),
        );
    }
    problems
}

/// Returns the message of a diagnostic about a module-scope item of the rule's text named `name`, a WGSL built-in.
fn builtin_name_message(name: &str) -> String {
    format!("`{name}` is a predeclared name of WGSL, and a rule's module-scope items cannot take it")
}

/// Returns the name a declaration keyword ending at byte `after` of `code` declares, with its offset.
///
/// A `var` can carry `<...>` before its name. `rest` are the words after the keyword.
fn declared_name<'a>(code: &str, after: usize, rest: &[(usize, &'a str)]) -> Option<(usize, &'a str)> {
    let mut position = after;
    let bytes = code.as_bytes();
    while bytes.get(position).is_some_and(u8::is_ascii_whitespace) {
        position += 1;
    }
    if bytes.get(position) == Some(&b'<') {
        let mut depth = 0usize;
        while let Some(&byte) = bytes.get(position) {
            position += 1;
            match byte {
                b'<' => depth += 1,
                b'>' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    rest.iter().find(|(offset, _)| *offset >= position).copied()
}

/// Returns `text` with every comment replaced by spaces, byte for byte, line breaks kept.
fn blank_comments(text: &str) -> String {
    let mut out = text.as_bytes().to_vec();
    for line in code_lines(text) {
        out[line.start..line.start + line.code.len()].copy_from_slice(line.code.as_bytes());
    }
    // Each comment byte became an ASCII space and every other character was copied whole.
    String::from_utf8(out).unwrap_or_default()
}

/// Checks the composed module of a rule: the functions and variables of its text, and every function they call.
///
/// `start_offset` is the entry's offset in the module's spans, from [`Composition::entry_start_offset`]. Each problem
/// is placed in the rule's text.
pub(crate) fn check_module(composition: &Composition, start_offset: usize) -> Vec<CodeDiagnostic> {
    let module = &composition.module;
    let mut walker = Walker {
        module,
        composition,
        start_offset,
        problems: Vec::new(),
        verdicts: HashMap::new(),
    };
    walker.check_globals();
    walker.check_builtin_names();
    for (handle, constant) in module.constants.iter() {
        let Some(located) = walker.locate(module.constants.get_span(handle)) else {
            continue;
        };
        if let Some(problem) = type_problem(module, &module.types[constant.ty].inner) {
            let name = constant.name.as_deref().unwrap_or("a constant");
            walker
                .problems
                .push(located.place(CodeDiagnostic::error(format!("`{name}` {problem}"))));
        }
    }
    for entry_point in &module.entry_points {
        if entry_point.name != "main" {
            let span = entry_point
                .function
                .body
                .span_iter()
                .map(|(_, span)| *span)
                .chain(
                    entry_point
                        .function
                        .expressions
                        .iter()
                        .map(|(handle, _)| entry_point.function.expressions.get_span(handle)),
                )
                .find(|span| walker.locate(*span).is_some())
                .unwrap_or_default();
            walker.report(
                span,
                format!(
                    "the rule declares the entry point `{}`. A grid rule declares none, and the engine's `main` calls \
                     `step_cell`",
                    entry_point.name
                ),
            );
        }
    }
    let mut own = Vec::new();
    for (handle, function) in module.functions.iter() {
        if walker.locate(module.functions.get_span(handle)).is_none() {
            continue;
        }
        own.push(handle);
        walker.check_types(handle, function);
        walker.check_buffers(function);
        let mut findings = Vec::new();
        walker.walk(&function.body, function, true, &mut findings);
        for (span, finding) in findings {
            walker.report(span, finding.message());
        }
    }
    walker.check_cost(&own);
    walker.problems
}

/// Returns the reason a value or variable of type `inner` is refused, or `None` when it is allowed.
fn type_problem(module: &Module, inner: &TypeInner) -> Option<String> {
    if contains_atomic(module, inner) {
        return Some("holds an atomic, and a grid rule uses no atomics".to_owned());
    }
    let bytes = type_bytes(module, inner);
    (bytes > MAX_FUNCTION_BYTES).then(|| {
        format!("takes {bytes} bytes, and a grid rule's values and variables take at most {MAX_FUNCTION_BYTES}")
    })
}

/// Returns the byte size of a value of type `inner`, or `u64::MAX` for a size past naga's limit.
fn type_bytes(module: &Module, inner: &TypeInner) -> u64 {
    inner.try_size(module.to_ctx()).map_or(u64::MAX, u64::from)
}

/// Returns whether `inner` is or holds an atomic.
fn contains_atomic(module: &Module, inner: &TypeInner) -> bool {
    match inner {
        TypeInner::Atomic(_) => true,
        TypeInner::Array { base, .. } | TypeInner::BindingArray { base, .. } | TypeInner::Pointer { base, .. } => {
            contains_atomic(module, &module.types[*base].inner)
        }
        TypeInner::Struct { members, .. } => members
            .iter()
            .any(|member| contains_atomic(module, &module.types[member.ty].inner)),
        _ => false,
    }
}

/// A statement a rule's functions cannot hold.
#[derive(Debug, Clone)]
enum Finding {
    Loop,
    /// A write into the named variable. The cell does not own it.
    Write(String),
    /// A pointer into the named variable, passed to a function. The cell does not own the variable.
    Pointer(String),
    Atomic,
    Barrier,
    /// Any other statement outside the ones a step needs.
    Statement,
}

impl Finding {
    /// Returns the message of a finding in the rule's own function.
    fn message(&self) -> String {
        match self {
            Self::Loop => "loops are not allowed in a grid rule. Without them a step's work is bounded".to_owned(),
            Self::Write(name) => {
                format!("this writes `{name}`. A grid rule writes only its locals and `private` variables")
            }
            Self::Pointer(name) => format!(
                "this passes a pointer to `{name}`. A grid rule passes pointers only to its locals and `private` \
                 variables"
            ),
            Self::Atomic => "atomics are not allowed in a grid rule".to_owned(),
            Self::Barrier => "barriers are not allowed in a grid rule".to_owned(),
            Self::Statement => "this statement is not allowed in a grid rule".to_owned(),
        }
    }

    /// Returns the message of a finding in `callee`, a function outside the rule's text that the rule calls.
    fn message_through(&self, callee: &str) -> String {
        let what = match self {
            Self::Loop => "holds a loop".to_owned(),
            Self::Write(name) => format!("writes `{name}`, memory the cell does not own"),
            Self::Pointer(name) => format!("passes a pointer to `{name}`, memory the cell does not own"),
            Self::Atomic => "uses atomics".to_owned(),
            Self::Barrier => "uses a barrier".to_owned(),
            Self::Statement => "holds a statement a grid rule cannot use".to_owned(),
        };
        format!("this calls `{callee}`, which {what}. A grid rule cannot call it")
    }
}

/// Walks the functions of a rule's text and the functions they call.
struct Walker<'a> {
    module: &'a Module,
    composition: &'a Composition,
    start_offset: usize,
    problems: Vec<CodeDiagnostic>,
    /// First finding in each function outside the rule's text that the rule calls, or `None` for a function without a
    /// finding.
    verdicts: HashMap<Handle<Function>, Option<Finding>>,
}

impl Walker<'_> {
    /// Returns the place of `span` in the rule's text, or `None` when it lies elsewhere.
    fn locate(&self, span: Span) -> Option<Located> {
        self.composition.locate_in_entry(span, self.start_offset)
    }

    /// Adds a problem at `span` of the rule's text, or with no place when `span` lies elsewhere.
    fn report(&mut self, span: Span, message: String) {
        let diagnostic = CodeDiagnostic::error(message);
        let diagnostic = match self.locate(span) {
            Some(located) => located.place(diagnostic),
            None => diagnostic,
        };
        self.problems.push(diagnostic);
    }

    /// Checks the module-scope variables of the rule's text, and the bytes of every `private` variable of the
    /// shader.
    fn check_globals(&mut self) {
        let module = self.module;
        // The template's and the shared modules' `private` variables count first.
        let mut private_bytes: u64 = module
            .global_variables
            .iter()
            .filter(|(handle, global)| {
                global.space == AddressSpace::Private
                    && self.locate(module.global_variables.get_span(*handle)).is_none()
            })
            .map(|(_, global)| type_bytes(module, &module.types[global.ty].inner))
            .fold(0, u64::saturating_add);
        let mut private_reported = false;
        for (handle, global) in module.global_variables.iter() {
            let Some(located) = self.locate(module.global_variables.get_span(handle)) else {
                continue;
            };
            let name = global.name.as_deref().unwrap_or("a variable");
            let inner = &module.types[global.ty].inner;
            if global.binding.is_some() {
                self.problems.push(located.place(CodeDiagnostic::error(format!(
                    "`{name}` has a binding. A grid rule declares no bindings"
                ))));
            } else if global.space != AddressSpace::Private {
                self.problems.push(located.place(CodeDiagnostic::error(format!(
                    "`{name}` is a module-scope variable outside `private`. A grid rule shares no memory between cells"
                ))));
            } else {
                private_bytes = private_bytes.saturating_add(type_bytes(module, inner));
                if private_bytes > MAX_PRIVATE_BYTES && !private_reported {
                    private_reported = true;
                    self.problems.push(located.place(CodeDiagnostic::error(format!(
                        "`{name}` brings the shader's `private` variables to {private_bytes} bytes, and they take at \
                         most {MAX_PRIVATE_BYTES}"
                    ))));
                    continue;
                }
            }
            if contains_atomic(module, inner) {
                self.problems.push(located.place(CodeDiagnostic::error(format!(
                    "`{name}` holds an atomic, and a grid rule uses no atomics"
                ))));
            }
        }
    }

    /// Refuses each function, constant, `private` variable and struct of the rule's text named as a WGSL built-in.
    ///
    /// [`check_text`] refuses the same names in the text. This check reads the names naga parsed.
    fn check_builtin_names(&mut self) {
        let module = self.module;
        let named = module
            .functions
            .iter()
            .map(|(handle, function)| (module.functions.get_span(handle), function.name.as_deref()))
            .chain(
                module
                    .constants
                    .iter()
                    .map(|(handle, constant)| (module.constants.get_span(handle), constant.name.as_deref())),
            )
            .chain(
                module
                    .global_variables
                    .iter()
                    .map(|(handle, global)| (module.global_variables.get_span(handle), global.name.as_deref())),
            )
            .chain(
                module
                    .types
                    .iter()
                    .map(|(handle, ty)| (module.types.get_span(handle), ty.name.as_deref())),
            );
        for (span, name) in named {
            let Some(name) = name.filter(|name| naga::keywords::wgsl::BUILTIN_IDENTIFIERS.contains(name)) else {
                continue;
            };
            if let Some(located) = self.locate(span) {
                self.problems
                    .push(located.place(CodeDiagnostic::error(builtin_name_message(name))));
            }
        }
    }

    /// Checks the local variables and computed values of `function`, reporting each problem once.
    ///
    /// A reference to a variable is left out, since its declaration is checked.
    fn check_types(&mut self, handle: Handle<Function>, function: &Function) {
        let name = function.name.as_deref().map_or_else(String::new, undecorate);
        let mut local_bytes: u64 = 0;
        let mut reported = HashSet::new();
        for (local, variable) in function.local_variables.iter() {
            let inner = &self.module.types[variable.ty].inner;
            let variable_name = variable.name.as_deref().unwrap_or("a local variable");
            let span = function.local_variables.get_span(local);
            if contains_atomic(self.module, inner) && reported.insert("atomic") {
                self.report(
                    span,
                    format!("`{variable_name}` holds an atomic, and a grid rule uses no atomics"),
                );
            }
            local_bytes = local_bytes.saturating_add(type_bytes(self.module, inner));
            if local_bytes > MAX_FUNCTION_BYTES && reported.insert("locals") {
                self.report(
                    span,
                    format!(
                        "`{variable_name}` brings the local variables of `{name}` to {local_bytes} bytes, and one \
                         function's take at most {MAX_FUNCTION_BYTES}"
                    ),
                );
            }
        }
        let info = &self.composition.info[handle];
        for (expression, value) in function.expressions.iter() {
            if matches!(value, Expression::GlobalVariable(_) | Expression::LocalVariable(_)) {
                continue;
            }
            let inner = info[expression].ty.inner_with(&self.module.types);
            if let Some(problem) = type_problem(self.module, inner)
                && reported.insert("value")
            {
                self.report(
                    function.expressions.get_span(expression),
                    format!("this value {problem}"),
                );
            }
        }
    }

    /// Refuses each engine buffer other than `params` that `function` names, once per buffer, at its first use.
    fn check_buffers(&mut self, function: &Function) {
        let mut reported = HashSet::new();
        for (expression, value) in function.expressions.iter() {
            let Expression::GlobalVariable(global) = value else {
                continue;
            };
            let variable = &self.module.global_variables[*global];
            let Some(name) = variable.name.as_deref() else {
                continue;
            };
            if variable.binding.is_some() && name != PARAMS_BUFFER && reported.insert(*global) {
                self.report(
                    function.expressions.get_span(expression),
                    format!(
                        "`{name}` is a buffer of the engine. A grid rule reads its cell through `cell` and the cells \
                         around it through `neighbor`"
                    ),
                );
            }
        }
    }

    /// Refuses a `step_cell` whose call runs more than [`MAX_STEP_COST`] expressions and statements.
    ///
    /// The problem is placed in the deepest function of the rule's text, among `own`, whose cost passes the limit
    /// while every function it calls stays within it, at the call that takes it past the limit.
    fn check_cost(&mut self, own: &[Handle<Function>]) {
        let module = self.module;
        let costs = function_costs(module);
        let cost = |handle: Handle<Function>| costs.get(handle.index()).copied().unwrap_or(u64::MAX);
        let Some(mut current) = own
            .iter()
            .copied()
            .find(|handle| module.functions[*handle].name.as_deref() == Some("step_cell"))
        else {
            return;
        };
        if cost(current) <= MAX_STEP_COST {
            return;
        }
        loop {
            let function = &module.functions[current];
            let mut calls = Vec::new();
            collect_calls(&function.body, &mut calls);
            if let Some(&(callee, _)) = calls
                .iter()
                .find(|(callee, _)| cost(*callee) > MAX_STEP_COST && own.contains(callee))
            {
                current = callee;
                continue;
            }
            let mut running = cost(current);
            for &(callee, _) in &calls {
                running = running.saturating_sub(cost(callee));
            }
            let span = calls
                .iter()
                .find(|(callee, _)| {
                    running = running.saturating_add(cost(*callee));
                    running > MAX_STEP_COST
                })
                .map_or_else(|| module.functions.get_span(current), |&(_, span)| span);
            let name = function.name.as_deref().map_or_else(String::new, undecorate);
            self.report(
                span,
                format!(
                    "one call of `{name}` runs {} expressions and statements, counting each function's body once per \
                     call, and a step runs at most {MAX_STEP_COST}",
                    cost(current)
                ),
            );
            return;
        }
    }

    /// Collects the findings of every statement of `block` in `function`, each with its span, into `findings`.
    ///
    /// `own` is whether `function` belongs to the rule's text, whose references to the engine's buffers
    /// [`Self::check_buffers`] refuses.
    fn walk(&mut self, block: &Block, function: &Function, own: bool, findings: &mut Vec<(Span, Finding)>) {
        for (statement, &span) in block.span_iter() {
            match statement {
                Statement::Emit(_) | Statement::Break | Statement::Continue | Statement::Return { .. } => {}
                Statement::Block(inner) => self.walk(inner, function, own, findings),
                Statement::If { accept, reject, .. } => {
                    self.walk(accept, function, own, findings);
                    self.walk(reject, function, own, findings);
                }
                Statement::Switch { cases, .. } => {
                    for case in cases {
                        self.walk(&case.body, function, own, findings);
                    }
                }
                Statement::Loop { .. } => findings.push((span, Finding::Loop)),
                Statement::Store { pointer, .. } => {
                    if let Some(name) = self.shared_target(function, *pointer, own) {
                        findings.push((span, Finding::Write(name)));
                    }
                }
                Statement::Call {
                    function: callee,
                    arguments,
                    ..
                } => {
                    // A callee may write through a pointer argument into function or private memory. The pointer
                    // has to reach memory the cell owns.
                    for &argument in arguments {
                        if let Some(name) = self.argument_target(function, argument, own) {
                            findings.push((span, Finding::Pointer(name)));
                        }
                    }
                    self.check_call(*callee, span);
                }
                Statement::Atomic { .. } | Statement::ImageAtomic { .. } => findings.push((span, Finding::Atomic)),
                Statement::ControlBarrier(_) | Statement::MemoryBarrier(_) | Statement::WorkGroupUniformLoad { .. } => {
                    findings.push((span, Finding::Barrier));
                }
                _ => findings.push((span, Finding::Statement)),
            }
        }
    }

    /// Returns the name of the variable that `pointer` writes into when the cell does not own it, or `None` for a
    /// local, a pointer argument into function or private memory, or a `private` variable of the rule's text.
    ///
    /// In a function of the rule's text (`own`), a buffer of the engine is left to [`Self::check_buffers`].
    fn shared_target(&self, function: &Function, pointer: Handle<Expression>, own: bool) -> Option<String> {
        let mut current = pointer;
        loop {
            match &function.expressions[current] {
                Expression::Access { base, .. } | Expression::AccessIndex { base, .. } => current = *base,
                Expression::FunctionArgument(index) => {
                    let argument = function.arguments.get(*index as usize)?;
                    let private = matches!(
                        self.module.types[argument.ty].inner,
                        TypeInner::Pointer {
                            space: AddressSpace::Function | AddressSpace::Private,
                            ..
                        }
                    );
                    return (!private)
                        .then(|| argument.name.clone().unwrap_or_else(|| "a pointer argument".to_owned()));
                }
                Expression::GlobalVariable(global) => return self.global_target(*global, own),
                _ => return None,
            }
        }
    }

    /// Returns the name of the variable that `argument`, an argument of a call in `function`, points into when the
    /// cell does not own it, or `None` for a value, a pointer argument or local of `function`, or a `private` variable
    /// of the rule's text.
    ///
    /// In a function of the rule's text (`own`), a buffer of the engine is left to [`Self::check_buffers`].
    fn argument_target(&self, function: &Function, argument: Handle<Expression>, own: bool) -> Option<String> {
        let mut current = argument;
        loop {
            match &function.expressions[current] {
                Expression::Access { base, .. } | Expression::AccessIndex { base, .. } => current = *base,
                Expression::GlobalVariable(global) => return self.global_target(*global, own),
                _ => return None,
            }
        }
    }

    /// Returns the name of `global` when the cell does not own it, or `None` for a `private` variable of the rule's
    /// text.
    ///
    /// In a function of the rule's text (`own`), a buffer of the engine is left to [`Self::check_buffers`].
    fn global_target(&self, global: Handle<naga::GlobalVariable>, own: bool) -> Option<String> {
        let variable = &self.module.global_variables[global];
        if own && variable.binding.is_some() {
            return None;
        }
        let in_text = self.locate(self.module.global_variables.get_span(global)).is_some();
        let name = variable
            .name
            .as_deref()
            .map_or_else(|| "a shared variable".to_owned(), undecorate);
        (variable.space != AddressSpace::Private || !in_text).then_some(name)
    }

    /// Checks a call at `at` to `callee`.
    ///
    /// A function of the rule's text is checked on its own, and the template's helpers are allowed. Any other
    /// function is walked once, and its first finding is reported at the call.
    fn check_call(&mut self, callee: Handle<Function>, at: Span) {
        let function = &self.module.functions[callee];
        if self.locate(self.module.functions.get_span(callee)).is_some()
            || function
                .name
                .as_deref()
                .is_some_and(|name| matches!(name, "neighbor" | "moore_count" | "von_neumann_count"))
        {
            return;
        }
        let verdict = if let Some(verdict) = self.verdicts.get(&callee) {
            verdict.clone()
        } else {
            let mut findings = Vec::new();
            self.walk(&function.body, function, false, &mut findings);
            let verdict = findings.into_iter().next().map(|(_, finding)| finding);
            self.verdicts.insert(callee, verdict.clone());
            verdict
        };
        if let Some(finding) = verdict {
            let name = function.name.as_deref().map_or_else(String::new, undecorate);
            self.report(at, finding.message_through(&name));
        }
    }
}

/// Returns the cost of one call of each function of `module`, in handle order: its expressions and statements, and
/// the cost of each function it calls once per call.
///
/// naga orders a module's functions so that each calls only functions before it, and WGSL has no recursion. A call
/// to a function that is not before its caller costs `u64::MAX`.
pub(crate) fn function_costs(module: &Module) -> Vec<u64> {
    let mut costs: Vec<u64> = Vec::with_capacity(module.functions.len());
    for (_, function) in module.functions.iter() {
        let mut cost = u64::try_from(function.expressions.len()).unwrap_or(u64::MAX);
        block_cost(&function.body, &costs, &mut cost);
        costs.push(cost);
    }
    costs
}

/// Adds the cost of every statement of `block` to `cost`, a call adding its callee's cost from `costs`.
fn block_cost(block: &Block, costs: &[u64], cost: &mut u64) {
    for statement in block {
        *cost = cost.saturating_add(1);
        match statement {
            Statement::Block(inner) => block_cost(inner, costs, cost),
            Statement::If { accept, reject, .. } => {
                block_cost(accept, costs, cost);
                block_cost(reject, costs, cost);
            }
            Statement::Switch { cases, .. } => {
                for case in cases {
                    block_cost(&case.body, costs, cost);
                }
            }
            Statement::Loop { body, continuing, .. } => {
                block_cost(body, costs, cost);
                block_cost(continuing, costs, cost);
            }
            Statement::Call { function, .. } => {
                *cost = cost.saturating_add(costs.get(function.index()).copied().unwrap_or(u64::MAX));
            }
            _ => {}
        }
    }
}

/// Collects every call in `block`, with its span, in the order the statements appear.
fn collect_calls(block: &Block, calls: &mut Vec<(Handle<Function>, Span)>) {
    for (statement, &span) in block.span_iter() {
        match statement {
            Statement::Block(inner) => collect_calls(inner, calls),
            Statement::If { accept, reject, .. } => {
                collect_calls(accept, calls);
                collect_calls(reject, calls);
            }
            Statement::Switch { cases, .. } => {
                for case in cases {
                    collect_calls(&case.body, calls);
                }
            }
            Statement::Loop { body, continuing, .. } => {
                collect_calls(body, calls);
                collect_calls(continuing, calls);
            }
            Statement::Call { function, .. } => calls.push((*function, span)),
            _ => {}
        }
    }
}
