//! Build-script support for Henad model crates.
//!
//! [`stamp_commit`] records the commit a crate was built from, a dirty flag and a hash of its sources, for
//! `henad::build_info!` to read. [`ShaderBuild`] generates the Rust bindings of a crate's WGSL shaders, from its build
//! script. Each shader is composed with the shared modules it reaches through `#import henad::<module>`, whose text
//! comes from henad-core's [`SHARED_WGSL_MODULES`]. henad-compute's `include_shaders!` then brings the two generated
//! files into the crate, as the modules `shader_bindings` and `binding_decls`.
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
//! therefore a Rust identifier and no keyword.
//!
//! A file with a `#define_import_path` line is a module other shaders import, and never an entry point. An import
//! resolves by file path alone. A module's import path mirrors its path below the shader root, or below the directory
//! of the file that imports it, as `#define_import_path gpu_vote::state` in `gpu_vote/state.wgsl`. The bindings name
//! an imported module by the import path it resolved through, and a quoted import path by the stem of its file name,
//! as `#import "std.inc"` gives the module `std`.
//!
//! The root `henad` is reserved for the shared modules, in any case. A file named `henad.wgsl` or a directory named
//! `henad` holding a `.wgsl` file would shadow them, and both are refused. So is a `.wgsl` path whose first component
//! is a name the generated bindings use at their root: `wgpu`, `bytemuck`, `std`, `core`, `alloc`, `_root`,
//! `ShaderEntry`, `layout_asserts` or `bytemuck_impls`, and so is an import that names a module after one of them.
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
//! fails the build. A compile-time assertion holds each list to the length of the layout `wgsl_bindgen` derives. A
//! module declares no binding, and a line holding `@binding` in one fails the build too.
//!
//! # Versions
//!
//! henad-build pins `wgsl_bindgen` to 0.23.3 and fixes the code it generates. Cargo keeps one release per
//! semver-compatible range in a lockfile. A build script that runs `wgsl_bindgen` itself therefore takes 0.23.3 as
//! well. Use the same 0.x of henad and henad-build. `include_shaders!` refuses to compile shaders composed against
//! other shared WGSL than the henad it links.
//!
//! [`SHARED_WGSL_MODULES`]: henad_core::authoring::primitives::wgsl::SHARED_WGSL_MODULES

#![cfg_attr(docsrs, feature(doc_cfg))]

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
/// Cargo rerun the script when any of them can change. `henad::build_info!` reads the four.
///
/// The commit comes from the package's `.cargo_vcs_info.json`, as in a registry download, or else from git when git
/// tracks the crate's `Cargo.toml`. Outside both the commit stays empty and the dirty flag unknown. The source hash is
/// computed in every case, and tells two builds apart where no commit can: an uncommitted edit, or a project not
/// under git. The dirty flag reads unknown in place of clean when git does not track the lockfile, since no commit
/// then records it.
///
/// A commit reruns the script once the crate sits in a git repository, even before git tracks the crate. A crate
/// built before `git init` keeps an empty commit until a file under `src` or the manifest changes, or
/// `cargo clean -p <package>` runs.
///
/// Note that the dirty flag and the source hash cover the files under `src`, the manifest and, outside a package, the
/// nearest `Cargo.lock`. Dotfiles, editor backups and the `.orig` and `.rej` files a merge or a patch leaves stay out
/// of both. A file a model reads at compile time, through `include_bytes!` or `include_str!`, belongs under `src` for
/// the stamp to see it. A symlink to a directory is not followed, and a shader [`ShaderBuild`] compiles from a linked
/// directory, or imports from outside `src`, changes neither. Data a model reads at run time from a path is recorded
/// by no stamp.
pub fn stamp_commit() {
    stamp::print(stamp::StampScope::Commit);
}

/// Sets the stamp of [`stamp_commit`] for henad-explore, whose stamp stands for the engine.
///
/// Git's answer is kept only inside Henad's own tree, where the dirty flag and the source hash cover henad-core,
/// henad-build, henad-compute and henad-explore with the workspace's lockfile. Also sets `HENAD_BUILD_CRATE_HASH`,
/// the hash of henad-explore's own `src` and manifest, and `HENAD_BUILD_STAMP_VERSION`, henad-build's own version.
#[doc(hidden)]
pub fn stamp_engine_commit() {
    stamp::print(stamp::StampScope::Engine);
}

/// Sets the source hash over the crate's own `src` and manifest, and the commit only from a `.cargo_vcs_info.json`.
///
/// Watches no git path and no lockfile. For henad-compute and henad-models.
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
    /// Whether the entry points were named one by one, through [`Self::entry_point`].
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
    /// Returns [`ShaderBuildError`] for a `.wgsl` file whose path below the root holds a component that is not a Rust
    /// identifier or is a keyword, for an entry point whose `ShaderEntry` variant is no Rust identifier (`self_.wgsl`
    /// gives `Self`), for two `.wgsl` files whose module paths, `ShaderEntry` variants or binding constants collide,
    /// for a file named `henad.wgsl` or a directory named `henad` holding a `.wgsl` file at any depth under the root,
    /// and for a root whose last component is `henad`. Files that are not `.wgsl` are never checked. Each error names
    /// the files.
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
    /// Note that the path is checked when [`Self::generate`] runs, as a discovered entry point is: it lies under the
    /// root, and each component of it is a Rust identifier and no keyword.
    pub fn entry_point(mut self, path: impl AsRef<Path>) -> Self {
        self.entries.push(path.as_ref().to_path_buf());
        self
    }

    /// Writes `shader_bindings.rs` and `binding_decls.rs` to `OUT_DIR`, each only when its bytes change.
    ///
    /// Prints a `cargo:rerun-if-changed` line for the shader root, which Cargo watches recursively, one for each entry
    /// point [`Self::entry_point`] added, and one for each file outside the root that an entry point imports. No line
    /// names a path under `OUT_DIR`.
    ///
    /// # Errors
    ///
    /// Returns [`ShaderBuildError`] for each refusal its variants list:
    ///
    /// - [`ShaderBuildError::Environment`] when `OUT_DIR` is not set, and [`ShaderBuildError::Io`] for a file that
    ///   cannot be read or written.
    /// - [`ShaderBuildError::ReservedName`] for a shader root whose last component is `henad` or a reserved name
    ///   under the root, and [`ShaderBuildError::ReservedImport`] for a module imported under one.
    /// - [`ShaderBuildError::OutsideRoot`] for an entry point outside the root, and [`ShaderBuildError::InvalidName`]
    ///   or [`ShaderBuildError::NameCollision`] for an entry point whose path gives no Rust name or another entry's.
    /// - [`ShaderBuildError::BindingLine`], [`ShaderBuildError::UnsupportedBinding`],
    ///   [`ShaderBuildError::ModuleBinding`] and [`ShaderBuildError::BindingGap`] for the bindings the reader refuses.
    /// - [`ShaderBuildError::Compose`] for a shader that does not compose.
    #[expect(clippy::print_stdout, reason = "a build script talks to Cargo through stdout")]
    pub fn generate(self) -> Result<(), ShaderBuildError> {
        let out_dir = std::env::var_os("OUT_DIR").ok_or(ShaderBuildError::Environment { variable: "OUT_DIR" })?;
        let report = self.generate_in(Path::new(&out_dir))?;
        for path in &report.watched {
            // The single-colon form. The double one needs Cargo 1.77 and would raise every downstream crate's MSRV.
            println!("cargo:rerun-if-changed={}", path.display());
        }
        for warning in &report.warnings {
            println!("cargo:warning={warning}");
        }
        Ok(())
    }

    /// Writes both files to `out_dir`, and returns the paths Cargo is to watch and the warnings to print.
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

/// Paths a finished generation has Cargo watch, the warnings it prints, and whether it ran the `wgsl_bindgen` pass.
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
    /// The same error names an entry point whose `ShaderEntry` variant is no Rust identifier, as `self_.wgsl` gives
    /// `Self`.
    InvalidName { path: PathBuf, component: String },
    /// Two shaders give one Rust name, or one shader's module holds another's.
    NameCollision {
        first: PathBuf,
        second: PathBuf,
        /// Kind of the name: `module`, `variant` for a `ShaderEntry` variant, or `constant` for a binding constant.
        kind: &'static str,
        name: String,
    },
    /// A file named `henad.wgsl`, a directory named `henad` holding a `.wgsl` file, or a shader root named `henad`,
    /// in any case. Or a `.wgsl` path starting with a name the generated bindings use, such as `wgpu` or `_root`.
    ReservedName { path: PathBuf, name: String },
    /// A file imported through an import path that makes it the module `name` at the root of the generated
    /// bindings, where `name` is `henad` in any case or a name the generated bindings use.
    ///
    /// The bindings name a quoted import after the stem of its file name. `#import "std.inc"` gives the module `std`.
    ReservedImport {
        path: PathBuf,
        import_path: String,
        name: String,
    },
    /// An entry point given to [`ShaderBuild::entry_point`] lies outside the shader root.
    OutsideRoot { path: PathBuf },
    /// A line holding `@binding` or `@group` in a form the binding parser does not read, for the reason given.
    BindingLine {
        path: PathBuf,
        /// Line number, counted from 1.
        line: usize,
        text: String,
        reason: &'static str,
    },
    /// A `@group(0)` binding of a kind no Henad pass binds: a sampler, a sampled texture, or an address space other
    /// than `uniform`, `storage, read` and `storage, read_write`.
    UnsupportedBinding {
        path: PathBuf,
        /// Line number, counted from 1.
        line: usize,
        text: String,
        /// Kind of the binding, as in "a sampled texture or a sampler".
        reason: &'static str,
    },
    /// A module, a file with a `#define_import_path` line, declares a binding on the line given.
    ModuleBinding {
        path: PathBuf,
        /// Line number, counted from 1.
        line: usize,
        text: String,
    },
    /// The `@group(0)` indices of a shader, sorted, do not run from 0 with no gap or repeat.
    BindingGap { path: PathBuf, indices: Vec<u32> },
    /// `wgsl_bindgen` could not compose the shaders or generate their bindings.
    Compose { message: String },
    /// A file or directory could not be read or written.
    Io { path: PathBuf, source: std::io::Error },
    /// Cargo did not set a variable a build script reads.
    Environment { variable: &'static str },
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
