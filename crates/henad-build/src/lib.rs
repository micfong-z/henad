//! Build-script support for Henad model crates.
//!
//! [`stamp_commit`] records the commit a crate was built from, a dirty flag and a hash of its sources, for
//! `henad::build_info!` to read. [`ShaderBuild`] generates the Rust bindings of a crate's WGSL shaders, from its build
//! script. Each shader is composed with the shared modules it reaches through `#import henad::<module>`, whose text
//! comes from henad-core's [`SHARED_WGSL_MODULES`]. `henad::include_shaders!` then brings the two generated files
//! into the crate, as the modules `shader_bindings` and `binding_decls`.
//!
//! ```no_run
//! // build.rs
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     henad_build::stamp_commit();
//!     henad_build::ShaderBuild::discover("src")?.generate()?;
//!     Ok(())
//! }
//! ```
//!
//! A crate without shaders keeps the build script for its stamp, and drops the `ShaderBuild` line.
//!
//! # Names
//!
//! A shader's path below the shader root becomes three Rust names. `gpu_vote/step.wgsl` gives the module
//! `shader_bindings::gpu_vote::step`, the `ShaderEntry` variant `GpuVoteStep` and the binding constant
//! `binding_decls::bindings::GPU_VOTE_STEP`. The constant is the path's components upper-cased and joined by `_`, with
//! the final `.wgsl` removed, so `my_wgsl/step.wgsl` gives `MY_WGSL_STEP`. Every component of a `.wgsl` file's path is
//! therefore a Rust identifier and not a keyword.
//!
//! A file with a `#define_import_path` line is a module other shaders import, and never an entry point. An import
//! resolves by file path alone. A module's import path mirrors its path below the shader root, or below the directory
//! of the file that imports it, as `#define_import_path gpu_vote::state` in `gpu_vote/state.wgsl`. The bindings refer
//! to an imported module by the import path it resolved through, and to a quoted import by the stem of its file name,
//! as `#import "std.inc"` gives the module `std`.
//!
//! The root `henad` is reserved for the shared modules, regardless of case. A file named `henad.wgsl` or a directory
//! named `henad` holding a `.wgsl` file would shadow them, and both are rejected. So is a `.wgsl` path whose first
//! component is a name the generated bindings use at their root: `wgpu`, `bytemuck`, `std`, `core`, `alloc`, `_root`,
//! `ShaderEntry`, `layout_asserts` or `bytemuck_impls`. An import is rejected too when its module path in the bindings
//! starts with `henad` or one of those names.
//!
//! Note that each struct's layout assertion is named after the struct's module path and name in upper snake case.
//! `gpu_vote::step::TallyParams` and `gpu_vote::step_tally::Params` both give
//! `GPU_VOTE_STEP_TALLY_PARAMS_ASSERTS`, and rustc reports the two inside the generated file. Rename one of the
//! structs.
//!
//! # Bindings
//!
//! `binding_decls::bindings` holds each entry point's `@group(0)` declarations, in `@binding` order, read from lines
//! of one form: `@group(0) @binding(N) var<...> name: Type;`. A line holding `@binding` or `@group` in any other form
//! fails the build. A compile-time assertion checks each list against the length of the layout `wgsl_bindgen`
//! derives. A module declares no bindings, and a line holding `@binding` in a module fails the build too.
//!
//! # Versions
//!
//! henad-build pins `wgsl_bindgen` to 0.23.3 and fixes the code it generates. Cargo keeps one release per
//! semver-compatible range in a lockfile. A build script that runs `wgsl_bindgen` itself therefore uses 0.23.3 as
//! well. Use the same 0.x of henad and henad-build. `include_shaders!` fails the build when the shaders were composed
//! against different shared WGSL than the linked henad provides.
//!
//! [`SHARED_WGSL_MODULES`]: henad_core::authoring::primitives::wgsl::SHARED_WGSL_MODULES

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]

mod binding_lines;
mod output;
mod paths;
mod stamp;

#[cfg(test)]
mod tests;

use std::fmt;
use std::path::{Path, PathBuf};

/// Stamps the crate whose build script calls it with its commit, a dirty flag and a hash of its sources.
///
/// Sets `HENAD_BUILD_COMMIT`, `HENAD_BUILD_COMMIT_DATE`, `HENAD_BUILD_DIRTY` and `HENAD_BUILD_SOURCE_HASH`, and has
/// Cargo rerun the script when any of them can change. `henad::build_info!` reads all four variables.
///
/// The commit comes from the package's `.cargo_vcs_info.json`, as in a registry download, or else from git when git
/// tracks the crate's `Cargo.toml`. Otherwise the commit stays empty and the dirty flag unknown. The source hash is
/// computed in every case, and tells two builds apart where no commit can: an uncommitted edit, or a project not
/// under git. The dirty flag is unknown instead of clean when git does not track the lockfile, because no commit
/// then records it.
///
/// A commit reruns the script once the crate sits in a git repository, even before git tracks the crate. A crate
/// built before `git init` keeps an empty commit until a file under `src` or the manifest changes, or
/// `cargo clean -p <package>` runs.
///
/// Note that the dirty flag and the source hash cover the files under `src`, the manifest and, outside a package, the
/// nearest `Cargo.lock`. Dotfiles, editor backups and the `.orig` and `.rej` files that a merge or a patch leaves
/// behind are excluded. A file a model reads at compile time, through `include_bytes!` or `include_str!`, belongs
/// under `src` for the stamp to see it. A symlink to a directory is not followed. A shader that [`ShaderBuild`]
/// compiles from a linked directory, or imports from outside `src`, changes neither the dirty flag nor the source
/// hash. No stamp records data that a model reads from a path at run time.
pub fn stamp_commit() {
    stamp::print(stamp::StampScope::Commit);
}

/// Sets the stamp of [`stamp_commit`] for henad-explore, whose stamp represents the engine.
///
/// The commit and the dirty flag come from git only inside Henad's own tree, where the dirty flag and the source hash
/// cover henad-core, henad-build, henad-compute and henad-explore with the workspace's lockfile. Also sets
/// `HENAD_BUILD_CRATE_HASH`, the hash of henad-explore's own `src` and manifest, and `HENAD_BUILD_STAMP_VERSION`,
/// henad-build's own version.
#[doc(hidden)]
pub fn stamp_engine_commit() {
    stamp::print(stamp::StampScope::Engine);
}

/// Sets the source hash over the crate's own `src` and manifest, and the commit only from a `.cargo_vcs_info.json`.
///
/// Watches no git path and no lockfile. henad-compute and henad-models call it.
#[doc(hidden)]
pub fn stamp_source_hash() {
    stamp::print(stamp::StampScope::SourceHash);
}

/// One crate's shader-binding generation, run from its build script.
#[derive(Debug, Clone)]
pub struct ShaderBuild {
    root: PathBuf,
    /// Entry points, relative to `root` or absolute.
    entries: Vec<PathBuf>,
    /// Whether the entry points were added one by one through [`Self::entry_point`].
    explicit: bool,
}

impl ShaderBuild {
    /// Returns a build of every `.wgsl` file under `shader_root` without a `#define_import_path` line.
    ///
    /// A symlink to a directory is followed, and a directory reached twice is walked once. Note that the build stamp of
    /// [`stamp_commit`] follows no such link, and an edit to a shader in a linked directory changes no stamp.
    ///
    /// # Errors
    ///
    /// Returns [`ShaderBuildError`] that lists the files. Files that are not `.wgsl` are never checked.
    ///
    /// - [`ShaderBuildError::InvalidName`] for a `.wgsl` path below the root with a component that is not a Rust
    ///   identifier or is a keyword, and for an entry point whose `ShaderEntry` variant is not a Rust identifier
    ///   (`self_.wgsl` gives `Self`).
    /// - [`ShaderBuildError::NameCollision`] for two entry points whose module paths, `ShaderEntry` variants or
    ///   binding constants collide, and for an entry point whose module contains another entry point's module.
    /// - [`ShaderBuildError::ReservedName`] for a file named `henad.wgsl` or a directory named `henad` holding a
    ///   `.wgsl` file at any depth under the root, for a `.wgsl` path starting with a name the generated bindings use,
    ///   and for a root whose last component is `henad`.
    /// - [`ShaderBuildError::Io`] for a file or directory that cannot be read.
    pub fn discover(shader_root: impl AsRef<Path>) -> Result<Self, ShaderBuildError> {
        let root = shader_root.as_ref().to_path_buf();
        paths::check_root(&root)?;
        let files = paths::wgsl_files(&root)?;
        paths::check_reserved(&root, &files)?;
        let mut entries = Vec::new();
        for file in &files {
            paths::check_components(&root, file)?;
            let path = root.join(file);
            let source = read(&path)?;
            if !paths::is_module(&source) {
                entries.push(file.clone());
            }
        }
        paths::check_collisions(&root, &entries)?;
        Ok(Self {
            root,
            entries,
            explicit: false,
        })
    }

    /// Returns a build with no entry points until [`Self::entry_point`] adds them.
    pub fn new(shader_root: impl AsRef<Path>) -> Self {
        Self {
            root: shader_root.as_ref().to_path_buf(),
            entries: Vec::new(),
            explicit: true,
        }
    }

    /// Adds the entry point `path`, relative to the shader root.
    ///
    /// Note that the path is checked when [`Self::generate`] runs, in the same way as a discovered entry point: it lies
    /// under the root, and each of its components is a Rust identifier and not a keyword.
    pub fn entry_point(mut self, path: impl AsRef<Path>) -> Self {
        self.entries.push(path.as_ref().to_path_buf());
        self
    }

    /// Writes `shader_bindings.rs` and `binding_decls.rs` to `OUT_DIR`, each only when its bytes change.
    ///
    /// Prints a `cargo:rerun-if-changed` line for the shader root, which Cargo watches recursively, one for each entry
    /// point [`Self::entry_point`] added, and one for each file outside the root that an entry point imports. No line
    /// refers to a path under `OUT_DIR`.
    ///
    /// # Errors
    ///
    /// Returns [`ShaderBuildError`] in these cases:
    ///
    /// - [`ShaderBuildError::Environment`] when `OUT_DIR` is not set, and [`ShaderBuildError::Io`] for a file that
    ///   cannot be read or written.
    /// - [`ShaderBuildError::ReservedName`] for a shader root whose last component is `henad` or a reserved name
    ///   under the root, and [`ShaderBuildError::ReservedImport`] for a module imported under a reserved name.
    /// - [`ShaderBuildError::OutsideRoot`] for an entry point outside the root, and [`ShaderBuildError::InvalidName`]
    ///   or [`ShaderBuildError::NameCollision`] for an entry point whose path gives no Rust name or repeats another
    ///   entry's name.
    /// - [`ShaderBuildError::BindingLine`], [`ShaderBuildError::UnsupportedBinding`],
    ///   [`ShaderBuildError::ModuleBinding`] and [`ShaderBuildError::BindingGap`] for bindings that the reader rejects.
    /// - [`ShaderBuildError::Compose`] for a shader that does not compose.
    #[expect(clippy::print_stdout, reason = "a build script talks to Cargo through stdout")]
    pub fn generate(self) -> Result<(), ShaderBuildError> {
        let out_dir = std::env::var_os("OUT_DIR").ok_or(ShaderBuildError::Environment { variable: "OUT_DIR" })?;
        let report = self.generate_in(Path::new(&out_dir))?;
        for path in &report.watched {
            // Each line uses the single-colon form. The double-colon form needs Cargo 1.77 and would raise every
            // downstream crate's MSRV.
            println!("cargo:rerun-if-changed={}", path.display());
        }
        for warning in &report.warnings {
            println!("cargo:warning={warning}");
        }
        Ok(())
    }

    /// Writes both files to `out_dir`, and returns the paths for Cargo to watch and the warnings to print.
    fn generate_in(self, out_dir: &Path) -> Result<Report, ShaderBuildError> {
        let root = std::path::absolute(&self.root).map_err(|source| ShaderBuildError::Io {
            path: self.root.clone(),
            source,
        })?;
        paths::check_root(&root)?;
        let files = paths::wgsl_files(&root)?;
        paths::check_reserved(&root, &files)?;

        let mut entries = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            let relative = if entry.is_absolute() {
                let Ok(relative) = entry.strip_prefix(&root) else {
                    return Err(ShaderBuildError::OutsideRoot { path: entry.clone() });
                };
                relative.to_path_buf()
            } else {
                entry.clone()
            };
            paths::check_components(&root, &relative)?;
            entries.push(relative);
        }
        paths::check_collisions(&root, &entries)?;

        let mut sources = Vec::with_capacity(files.len());
        let mut warnings = Vec::new();
        for file in &files {
            let path = root.join(file);
            let source = read(&path)?;
            if paths::defines_reserved_path(&source) {
                warnings.push(format!(
                    "{} declares an import path under `henad::`, a root reserved for the shared modules. An import \
                     resolves by file path, and never reads this line.",
                    path.display()
                ));
            }
            if paths::is_module(&source) && !entries.contains(file) {
                binding_lines::refuse_bindings(&path, &source)?;
            }
            sources.push((file.clone(), source));
        }

        let generated = output::generate(&root, &entries, &sources, out_dir)?;

        let mut watched = vec![root.clone()];
        if self.explicit {
            watched.extend(entries.iter().map(|entry| root.join(entry)));
        }
        watched.extend(generated.outside_root);
        Ok(Report {
            watched,
            warnings,
            bound: generated.bound,
        })
    }
}

/// Result of a generation: the paths for Cargo to watch, the warnings to print, and whether the `wgsl_bindgen` pass
/// ran.
#[derive(Debug)]
struct Report {
    watched: Vec<PathBuf>,
    warnings: Vec<String>,
    #[cfg_attr(not(test), expect(dead_code, reason = "only the tests ask whether the pass ran"))]
    bound: bool,
}

/// Returns the text of `path`.
fn read(path: &Path) -> Result<String, ShaderBuildError> {
    std::fs::read_to_string(path).map_err(|source| ShaderBuildError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Reason a shader build fails.
///
/// `Debug` writes the same text as `Display`, so a build script that returns the error shows its guidance.
#[non_exhaustive]
pub enum ShaderBuildError {
    /// A component of a `.wgsl` file's path below the shader root is not a Rust identifier, or is a keyword.
    ///
    /// The same error reports an entry point whose `ShaderEntry` variant is not a Rust identifier, as `self_.wgsl`
    /// gives `Self`.
    InvalidName {
        /// Shader file.
        path: PathBuf,
        /// Path component or `ShaderEntry` variant that was rejected, or the whole relative path when one of its
        /// components is `.`, `..` or not valid Unicode.
        component: String,
    },
    /// Two shaders give the same Rust name, or one shader's module contains another shader's module.
    NameCollision {
        /// Earlier shader of the two, in entry order.
        first: PathBuf,
        /// Later shader of the two.
        second: PathBuf,
        /// Kind of the name: `module`, `variant` for a `ShaderEntry` variant, or `constant` for a binding constant.
        kind: &'static str,
        /// Name the two shaders share.
        name: String,
    },
    /// A shader path or root that uses a reserved name.
    ///
    /// The name `henad` is reserved regardless of case, for the last component of the shader root and for every
    /// component of a `.wgsl` file's path below it. A name the generated bindings use, such as `wgpu` or `_root`, is
    /// reserved for the first component of a `.wgsl` path.
    ReservedName {
        /// Shader root, or the shader file whose path uses the name.
        path: PathBuf,
        /// Reserved name, `henad` in lower case or a name the generated bindings use.
        name: String,
    },
    /// A file imported through an import path that makes it the module `name` at the root of the generated
    /// bindings, where `name` is `henad` in any letter case or a name the generated bindings use.
    ///
    /// The bindings name a quoted import after the stem of its file name. `#import "std.inc"` gives the module `std`.
    ReservedImport {
        /// Imported file.
        path: PathBuf,
        /// Import path that reached the file, with its quotes when quoted.
        import_path: String,
        /// First component of the module path the bindings give the file.
        name: String,
    },
    /// An entry point given to [`ShaderBuild::entry_point`] lies outside the shader root.
    OutsideRoot {
        /// Absolute path of the entry point, as given.
        path: PathBuf,
    },
    /// A line holding `@binding` or `@group` in a form that the binding parser does not accept.
    BindingLine {
        /// Entry point the line belongs to.
        path: PathBuf,
        /// Line number, counted from 1.
        line: usize,
        /// Text of the line, trimmed and without its comments.
        text: String,
        /// Reason the line is rejected, as in "`var<` has no closing `>`".
        reason: &'static str,
    },
    /// A `@group(0)` binding of a kind no Henad pass binds: a sampler, a sampled texture, or an address space other
    /// than `uniform`, `storage, read` and `storage, read_write`.
    UnsupportedBinding {
        /// Entry point the line belongs to.
        path: PathBuf,
        /// Line number, counted from 1.
        line: usize,
        /// Text of the line, trimmed and without its comments.
        text: String,
        /// Kind of the binding, as in "a sampled texture or a sampler".
        reason: &'static str,
    },
    /// A file that is not an entry point declares a binding on the given line.
    ///
    /// Such a file is a module, with a `#define_import_path` line, or another file a shader imports.
    ModuleBinding {
        /// Module or imported file.
        path: PathBuf,
        /// Line number, counted from 1.
        line: usize,
        /// Text of the line, trimmed and without its comments.
        text: String,
    },
    /// The `@group(0)` indices of a shader, sorted, do not run from 0 with no gap or repeat.
    BindingGap {
        /// Entry point the indices belong to.
        path: PathBuf,
        /// `@binding` indices of the shader's `@group(0)` lines, sorted.
        indices: Vec<u32>,
    },
    /// `wgsl_bindgen` could not compose the shaders or generate their bindings.
    Compose {
        /// Message of the `wgsl_bindgen` error or panic, or a message that lists the files of an import cycle.
        message: String,
    },
    /// A file or directory could not be read or written.
    Io {
        /// File or directory the operation failed on.
        path: PathBuf,
        /// Error from the operating system.
        source: std::io::Error,
    },
    /// Cargo did not set a variable a build script reads.
    Environment {
        /// Name of the variable, as in `OUT_DIR`.
        variable: &'static str,
    },
}

impl fmt::Display for ShaderBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName { path, component } => {
                if component.is_empty() {
                    write!(f, "{}: the path gives an empty Rust name. ", path.display())?;
                } else {
                    write!(
                        f,
                        "{}: `{component}` is not a Rust identifier, or is a keyword. ",
                        path.display()
                    )?;
                }
                f.write_str(
                    "Each component of a shader's path below the shader root becomes part of a Rust name, and the \
                     components joined in PascalCase name the shader's `ShaderEntry` variant",
                )
            }
            Self::NameCollision {
                first,
                second,
                kind,
                name,
            } => write!(
                f,
                "{} and {} both give the {kind} `{name}`",
                first.display(),
                second.display()
            ),
            Self::ReservedName { path, name } if name.eq_ignore_ascii_case("henad") => write!(
                f,
                "{}: the name `henad`, in any case, is reserved for the shared modules, and a shader file or \
                 directory of that name would shadow them",
                path.display()
            ),
            Self::ReservedName { path, name } => write!(
                f,
                "{}: the generated bindings use the name `{name}` at their root, and a shader path starting with it \
                 would shadow it",
                path.display()
            ),
            Self::ReservedImport {
                path,
                import_path,
                name,
            } => {
                write!(
                    f,
                    "{}: the import path `{import_path}` makes this file the module `{name}`. ",
                    path.display()
                )?;
                if name.eq_ignore_ascii_case("henad") {
                    f.write_str("The name `henad`, in any case, is reserved for the shared modules. ")?;
                } else {
                    f.write_str("The generated bindings use that name at their root. ")?;
                }
                f.write_str("Import the file under another path")
            }
            Self::OutsideRoot { path } => write!(f, "{}: an entry point lies outside the shader root", path.display()),
            Self::BindingLine {
                path,
                line,
                text,
                reason,
            } => write!(
                f,
                "{}:{line}: {reason} in `{text}`. Write each binding on one line, as \
                 `@group(0) @binding(N) var<...> name: Type;`",
                path.display()
            ),
            Self::UnsupportedBinding {
                path,
                line,
                text,
                reason,
            } => write!(
                f,
                "{}:{line}: `{text}` binds {reason}. Group 0 of an entry point holds storage buffers, uniforms and \
                 storage textures. Move a render shader out of the shader root, or name the entry points with \
                 `ShaderBuild::new`",
                path.display()
            ),
            Self::ModuleBinding { path, line, text } => write!(
                f,
                "{}:{line}: `{text}` declares a binding in a module another shader imports. Bindings belong in the \
                 entry shader",
                path.display()
            ),
            Self::BindingGap { path, indices } => write!(
                f,
                "{}: the @group(0) @binding indices {indices:?} do not run from 0 with no gap or repeat",
                path.display()
            ),
            Self::Compose { message } => write!(f, "shader composition failed: {message}"),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Environment { variable } => write!(f, "`{variable}` is not set. Call this from a build script"),
        }
    }
}

impl fmt::Debug for ShaderBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for ShaderBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
