//! The reader of a shader's `@group(0)` declarations, one line each.
//!
//! `wgsl_bindgen` keeps binding names only in doc comments, where the engine cannot read them. Everything the engine
//! needs, the index, the name, the address space and the access, sits on the declaration line itself. Only a binding's
//! type needs the imports resolved, and the reader never composes the module.

use std::path::Path;

use henad_core::authoring::model::binding::BindingKind;

use crate::ShaderBuildError;

/// One `@group(0)` declaration of a shader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Binding {
    pub(crate) index: u32,
    pub(crate) name: String,
    pub(crate) kind: BindingKind,
}

/// Returns the `@group(0)` declarations of `source`, the text of the shader at `path`, in `@binding` order.
///
/// Declarations of other groups are read and left out.
///
/// # Errors
///
/// Returns [`ShaderBuildError::BindingLine`] for a line that holds `@binding(` or opens with `@group(` in another form
/// than `@group(G) @binding(N) var<...> name: Type;`, and for a binding kind the engine does not bind.
/// Returns [`ShaderBuildError::BindingGap`] when the indices do not run from 0 with no gap or repeat.
pub(crate) fn read_bindings(path: &Path, source: &str) -> Result<Vec<Binding>, ShaderBuildError> {
    let mut bindings = Vec::new();
    for (number, line) in strip_block_comments(source).lines().enumerate() {
        let code = line.split("//").next().unwrap_or_default().trim();
        if !code.contains("@binding(") && !code.starts_with("@group(") {
            continue;
        }
        let refuse = |reason| ShaderBuildError::BindingLine {
            path: path.to_path_buf(),
            line: number + 1,
            text: code.to_owned(),
            reason,
        };
        if let Some(binding) = read_line(code).map_err(refuse)? {
            bindings.push(binding);
        }
    }
    bindings.sort_by_key(|binding| binding.index);
    let indices: Vec<u32> = bindings.iter().map(|binding| binding.index).collect();
    if indices.iter().zip(0..).any(|(&index, slot)| index != slot) {
        return Err(ShaderBuildError::BindingGap {
            path: path.to_path_buf(),
            indices,
        });
    }
    Ok(bindings)
}

/// Returns an error for the first line of `source`, the text of the module at `path`, that holds `@binding(`.
///
/// # Errors
///
/// Returns [`ShaderBuildError::ModuleBinding`] naming that line.
pub(crate) fn refuse_bindings(path: &Path, source: &str) -> Result<(), ShaderBuildError> {
    for (number, line) in strip_block_comments(source).lines().enumerate() {
        let code = line.split("//").next().unwrap_or_default().trim();
        if code.contains("@binding(") {
            return Err(ShaderBuildError::ModuleBinding {
                path: path.to_path_buf(),
                line: number + 1,
                text: code.to_owned(),
            });
        }
    }
    Ok(())
}

/// Returns the declaration on `code`, one line with its comment removed, or `None` for one of another group.
fn read_line(code: &str) -> Result<Option<Binding>, &'static str> {
    let (group, rest) = attribute(code, "@group(").ok_or("the line does not open with `@group(G)`")?;
    let (index, rest) = attribute(rest, "@binding(").ok_or("`@group(G)` is not followed by `@binding(N)`")?;
    let rest = rest
        .strip_prefix("var")
        .ok_or("`@binding(N)` is not followed by `var`")?;
    let (space, rest) = match rest.strip_prefix('<') {
        Some(rest) => {
            let (space, rest) = rest.split_once('>').ok_or("`var<` has no closing `>`")?;
            (Some(space), rest)
        }
        None => (None, rest),
    };
    let (name, kind_text) = rest.split_once(':').ok_or("the name is not followed by `:`")?;
    let name = name.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err("the binding has no plain name");
    }
    let binding_type = kind_text
        .trim()
        .strip_suffix(';')
        .ok_or("the declaration does not end with `;` on the same line")?;
    if group != 0 {
        return Ok(None);
    }
    Ok(Some(Binding {
        index,
        name: name.to_owned(),
        kind: kind(space, binding_type.trim())?,
    }))
}

/// Returns the number inside the attribute `opening` at the start of `code`, and the text after it.
fn attribute<'a>(code: &'a str, opening: &str) -> Option<(u32, &'a str)> {
    let rest = code.trim_start().strip_prefix(opening)?;
    let (number, rest) = rest.split_once(')')?;
    Some((number.trim().parse().ok()?, rest.trim_start()))
}

/// Returns the kind of a binding with the address space `space`, the text inside `var<...>`, and the type
/// `binding_type`.
fn kind(space: Option<&str>, binding_type: &str) -> Result<BindingKind, &'static str> {
    let Some(space) = space else {
        return if binding_type.starts_with("texture_storage_") {
            Ok(BindingKind::StorageTexture)
        } else {
            Err("a sampled texture or a sampler, which no Henad pass binds")
        };
    };
    let parts: Vec<&str> = space.split(',').map(str::trim).collect();
    match parts.as_slice() {
        ["uniform"] => Ok(BindingKind::Uniform),
        ["storage"] | ["storage", "read"] => Ok(BindingKind::Storage { read_only: true }),
        ["storage", "read_write"] => Ok(BindingKind::Storage { read_only: false }),
        _ => Err("an address space other than `uniform`, `storage, read` or `storage, read_write`"),
    }
}

/// Returns `source` with each block comment, nested ones included, replaced by spaces and the newlines it held.
fn strip_block_comments(source: &str) -> String {
    let mut stripped = String::with_capacity(source.len());
    let mut depth = 0usize;
    let mut characters = source.chars().peekable();
    while let Some(character) = characters.next() {
        match (character, characters.peek()) {
            // A line comment can hold `/*` and opens no block.
            ('/', Some('/')) if depth == 0 => {
                stripped.push(character);
                while let Some(&next) = characters.peek() {
                    if next == '\n' {
                        break;
                    }
                    stripped.push(next);
                    characters.next();
                }
            }
            ('/', Some('*')) => {
                characters.next();
                depth += 1;
                stripped.push_str("  ");
            }
            ('*', Some('/')) if depth > 0 => {
                characters.next();
                depth -= 1;
                stripped.push_str("  ");
            }
            ('\n', _) => stripped.push('\n'),
            _ if depth > 0 => stripped.push(' '),
            _ => stripped.push(character),
        }
    }
    stripped
}
