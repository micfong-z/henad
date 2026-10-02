//! Files a stamp hashes, and the `.cargo_vcs_info.json` of a package.

use std::path::{Path, PathBuf};

use henad_core::explore::fingerprint::Fnv1a64;

use super::git::{Repository, list_sources};

/// File `cargo package` writes into a package from a git checkout.
const VCS_INFO: &str = ".cargo_vcs_info.json";
/// Author's manifest in a package, beside the normalised `Cargo.toml` Cargo writes.
const ORIGINAL_MANIFEST: &str = "Cargo.toml.orig";

/// One crate's directory, and the label its files carry in a hash.
#[derive(Debug, Clone)]
pub(crate) struct CrateFiles {
    directory: PathBuf,
    /// Prefix of every label, the crate's directory below the repository root for the engine's crates and empty
    /// otherwise.
    label_prefix: String,
}

impl CrateFiles {
    pub(crate) fn new(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
            label_prefix: String::new(),
        }
    }

    /// Returns the crate `package` under `crates/` in `repository`, its files labelled from the repository root.
    pub(crate) fn in_repository(repository: &Repository, package: &str) -> Self {
        Self {
            directory: repository.toplevel().join("crates").join(package),
            label_prefix: format!("crates/{package}/"),
        }
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    /// Returns whether the crate is a package, as a registry download or a `cargo vendor` copy is.
    pub(crate) fn is_package(&self) -> bool {
        self.directory.join(ORIGINAL_MANIFEST).is_file()
    }

    /// Returns the files of the hash with their labels: every file under `src` the exclusions leave, and the
    /// manifest.
    ///
    /// Under git those are the files git tracks or does not ignore. The manifest is the author's, `Cargo.toml.orig`,
    /// in a package, and carries the label `Cargo.toml` either way.
    pub(crate) fn sources(&self, repository: Option<&Repository>) -> Vec<(String, PathBuf)> {
        let listed = match repository {
            Some(_) => list_sources(&self.directory),
            None => walk(&self.directory.join("src")),
        };
        let mut sources: Vec<(String, PathBuf)> = listed
            .into_iter()
            .filter(|below_src| !is_excluded(below_src))
            .map(|below_src| {
                let path = self.directory.join("src").join(&below_src);
                (format!("{}src/{below_src}", self.label_prefix), path)
            })
            .collect();
        let manifest = if self.is_package() {
            ORIGINAL_MANIFEST
        } else {
            "Cargo.toml"
        };
        sources.push((
            format!("{}Cargo.toml", self.label_prefix),
            self.directory.join(manifest),
        ));
        sources
    }

    /// Returns `src` and `Cargo.toml`, each when it exists.
    pub(crate) fn watched(&self) -> Vec<PathBuf> {
        ["src", "Cargo.toml"]
            .into_iter()
            .map(|name| self.directory.join(name))
            .filter(|path| path.exists())
            .collect()
    }

    /// Returns `src` and `Cargo.toml` as pathspecs relative to the root of `repository`.
    pub(crate) fn pathspecs(&self, repository: &Repository) -> Vec<String> {
        ["src", "Cargo.toml"]
            .into_iter()
            .filter_map(|name| repository.pathspec(&self.directory.join(name)))
            .collect()
    }
}

/// Returns whether a file is left out of the hash and the dirty flag, from its path below `src` with `/` separators.
///
/// A component that starts with `.` leaves the file out, as does a name ending in `~` or `.swp`. No kernel file is
/// a dotfile or a backup, and the files Finder and editors write stay out whether git ignores them or not.
pub(crate) fn is_excluded(below_src: &str) -> bool {
    let mut components = below_src.split('/');
    let name = below_src.rsplit('/').next().unwrap_or(below_src);
    components.any(|component| component.starts_with('.')) || name.ends_with('~') || name.ends_with(".swp")
}

/// Returns whether `path` is a symlink to something other than a file, or to nothing.
///
/// The hash leaves such a link out, as a dangling link or a link to a directory holds no source to hash, and the
/// dirty flag does the same.
pub(crate) fn is_link_to_no_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink())
        && !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

/// Returns every file under `src`, relative to it with `/` separators, by walking the directory.
///
/// A symlink to a file counts as the file. A symlink to a directory is not followed, as git does not follow one.
fn walk(src: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![(src.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = format!("{prefix}{name}");
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                pending.push((entry.path(), format!("{relative}/")));
            } else {
                found.push(relative);
            }
        }
    }
    found
}

/// Returns the FNV-1a hash of `sources`, in label order, each file's CRLF line endings read as LF.
///
/// A label is hashed beside its file's bytes. A file that is not there, or a symlink to something other than a file,
/// is left out. So is a file that disappears before it is read, as an editor's atomic save replaces one. Returns
/// `None` when a file that is there cannot be read.
pub(crate) fn hash_sources(mut sources: Vec<(String, PathBuf)>) -> Option<u64> {
    sources.sort_by(|left, right| left.0.cmp(&right.0));
    sources.dedup_by(|left, right| left.0 == right.0);
    let mut hasher = Fnv1a64::new();
    for (label, path) in &sources {
        if !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
            continue;
        }
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        let normalised = crlf_to_lf(&bytes);
        hasher.write_str(label);
        hasher.write_u64(normalised.len() as u64);
        hasher.write(&normalised);
    }
    Some(hasher.finish())
}

/// Returns `bytes` with every CRLF pair replaced by LF.
fn crlf_to_lf(bytes: &[u8]) -> Vec<u8> {
    let mut normalised = Vec::with_capacity(bytes.len());
    let mut iterator = bytes.iter().peekable();
    while let Some(&byte) = iterator.next() {
        if byte == b'\r' && iterator.peek() == Some(&&b'\n') {
            continue;
        }
        normalised.push(byte);
    }
    normalised
}

/// Returns the nearest `Cargo.lock` in `directory` or a directory above it.
pub(crate) fn nearest_lockfile(directory: &Path) -> Option<PathBuf> {
    directory
        .ancestors()
        .map(|ancestor| ancestor.join("Cargo.lock"))
        .find(|lockfile| lockfile.is_file())
}

/// Commit and dirty flag a package's `.cargo_vcs_info.json` records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VcsInfo {
    /// Full commit hash, `git.sha1`.
    pub(crate) sha1: String,
    /// `git.dirty`, false when the file leaves it out.
    pub(crate) dirty: bool,
}

impl VcsInfo {
    /// Reads the `.cargo_vcs_info.json` in `directory`, `None` when there is none or it names no commit.
    pub(crate) fn read(directory: &Path) -> Option<Self> {
        Self::parse(&std::fs::read_to_string(directory.join(VCS_INFO)).ok()?)
    }

    /// Reads the commit and the dirty flag from the text of a `.cargo_vcs_info.json`.
    ///
    /// The file is the small fixed object Cargo writes, and its two fields are found by their keys.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let sha1 = field_value(text, "sha1")?
            .strip_prefix('"')?
            .split('"')
            .next()?
            .to_owned();
        if sha1.is_empty() || !sha1.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let dirty = field_value(text, "dirty").is_some_and(|value| value.starts_with("true"));
        Some(Self { sha1, dirty })
    }

    /// Commit shortened to the eight characters `git rev-parse --short=8` prints.
    pub(crate) fn short_commit(&self) -> &str {
        self.sha1.get(..8).unwrap_or(&self.sha1)
    }
}

/// Returns the text after the colon that follows the quoted `key`, leading whitespace removed.
fn field_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let after_key = &text[text.find(&format!("\"{key}\""))? + key.len() + 2..];
    Some(after_key.trim_start().strip_prefix(':')?.trim_start())
}
