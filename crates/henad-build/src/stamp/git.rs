//! Git queries of a stamp: the commit, its date, the dirty flag, the files under `src` and the paths a commit
//! changes.
//!
//! Every query runs with `GIT_OPTIONAL_LOCKS=0`. A stamp then never takes `.git/index.lock` to refresh the index, and
//! a build never blocks a git command the user runs beside it.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::files::{is_excluded, is_link_to_no_file};

/// Git repository whose work tree holds a crate, seen from the crate's directory.
///
/// [`Repository::find`] returns a repository only when git tracks the crate's `Cargo.toml`.
#[derive(Debug, Clone)]
pub(crate) struct Repository {
    /// Crate's directory, where each query runs.
    directory: PathBuf,
    toplevel: PathBuf,
    /// Crate's directory below the root, with a trailing `/`, as `git rev-parse --show-prefix` prints it.
    prefix: String,
}

impl Repository {
    /// Returns the repository of the crate in `directory`, `None` when git does not track the crate's `Cargo.toml`.
    ///
    /// Note that a package unpacked inside an unrelated repository, for example under a `CARGO_HOME` inside that
    /// repository, is treated as outside git. Otherwise it would record that repository's commit.
    pub(crate) fn find(directory: &Path) -> Option<Self> {
        run(directory, &["ls-files", "--error-unmatch", "Cargo.toml"])?;
        Self::enclosing(directory)
    }

    /// Returns the repository whose work tree holds `directory`, whether or not git tracks anything in it.
    pub(crate) fn enclosing(directory: &Path) -> Option<Self> {
        let toplevel = PathBuf::from(text(directory, &["rev-parse", "--show-toplevel"])?);
        let prefix = text(directory, &["rev-parse", "--show-prefix"])?;
        Some(Self {
            directory: directory.to_path_buf(),
            toplevel,
            prefix,
        })
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(crate) fn toplevel(&self) -> &Path {
        &self.toplevel
    }

    pub(crate) fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Returns the first eight characters of `HEAD`'s hash, or empty text in a repository with no commit.
    ///
    /// The full hash is read and truncated. `--short=8` lengthens a hash that eight characters leave ambiguous.
    pub(crate) fn commit(&self) -> String {
        let full = text(&self.directory, &["rev-parse", "HEAD"]).unwrap_or_default();
        full.chars().take(8).collect()
    }

    /// Returns the date of `HEAD`'s commit as `YYYY-MM-DD`, or empty text when git prints no such date.
    ///
    /// `log.showSignature` is turned off for the query. Otherwise git prints the signature check before the date.
    pub(crate) fn commit_date(&self) -> String {
        let arguments = [
            "-c",
            "log.showSignature=false",
            "log",
            "-1",
            "--format=%cd",
            "--date=short",
        ];
        text(&self.directory, &arguments)
            .filter(|date| is_date(date))
            .unwrap_or_default()
    }

    /// Returns the pathspec of `path`, a file in the crate's directory or a directory above it, relative to the
    /// repository root with `/` separators. `None` when git does not track the file, or it lies above the root.
    ///
    /// The pathspec is built from the crate's prefix below the root, and no absolute path is compared.
    pub(crate) fn tracked(&self, path: &Path) -> Option<String> {
        let parent = path.parent()?;
        let levels = self.directory.ancestors().position(|ancestor| ancestor == parent)?;
        let name = path.file_name()?.to_str()?;
        let below_root: Vec<&str> = self.prefix.split('/').filter(|part| !part.is_empty()).collect();
        let kept = below_root.len().checked_sub(levels)?;
        let mut pathspec: String = below_root[..kept].iter().map(|part| format!("{part}/")).collect();
        pathspec.push_str(name);
        let from_root = format!(":(top,literal){pathspec}");
        run(
            &self.directory,
            &["ls-files", "--error-unmatch", "--", from_root.as_str()],
        )?;
        Some(pathspec)
    }

    /// Returns whether any path below `pathspecs` differs from `HEAD`, including untracked files that git does not
    /// ignore. `None` when git cannot tell, or `pathspecs` is empty.
    ///
    /// Each path is checked against the exclusions below the pathspec it falls under, and a symlink to a directory
    /// or to nothing is left out, as it is from the hash. An untracked `src/.DS_Store` or `src/step.rs.swp` leaves
    /// the tree clean.
    pub(crate) fn dirty(&self, pathspecs: &[String]) -> Option<bool> {
        // An empty list would query the whole repository, and the filter below would match no path.
        if pathspecs.is_empty() {
            return None;
        }
        let mut arguments = vec![
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
            "--",
        ];
        arguments.extend(pathspecs.iter().map(String::as_str));
        let output = run(&self.toplevel, &arguments)?;
        let output = String::from_utf8_lossy(&output);
        Some(
            output
                .split('\0')
                .filter_map(|record| record.get(3..))
                .filter(|path| !path.is_empty())
                .any(|path| {
                    pathspecs
                        .iter()
                        .any(|pathspec| match path.strip_prefix(pathspec.as_str()) {
                            Some("") => true,
                            Some(rest) => rest.strip_prefix('/').is_some_and(|below| {
                                !is_excluded(below) && !is_link_to_no_file(&self.toplevel.join(path))
                            }),
                            None => false,
                        })
                }),
        )
    }

    /// Returns the existing repository paths that change on every commit, reset and checkout.
    ///
    /// Each path comes from `git rev-parse --git-path` or `--git-common-dir`. Both options resolve a linked worktree's
    /// own files and the files it shares. `HEAD` and `logs/HEAD` belong to the worktree itself. The branch's loose ref
    /// and `packed-refs` sit in the common directory, as does `refs/heads`, watched instead of `logs/HEAD` when
    /// `core.logAllRefUpdates` is off. A repository in the reftable format has `reftable` instead of the refs.
    pub(crate) fn watched(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        paths.extend(self.git_path("HEAD"));
        let logs = self.git_path("logs/HEAD").filter(|logs| logs.is_file());
        let has_logs = logs.is_some();
        paths.extend(logs);
        if let Some(common) = self.common_dir() {
            let reftable = common.join("reftable");
            if reftable.is_dir() {
                paths.push(reftable);
            } else {
                let branch = text(&self.directory, &["symbolic-ref", "-q", "HEAD"]);
                paths.extend(branch.and_then(|branch| self.git_path(&branch)));
                paths.push(common.join("packed-refs"));
                if !has_logs {
                    paths.push(common.join("refs").join("heads"));
                }
            }
        }
        paths.retain(|path| path.exists());
        paths
    }

    /// Returns the absolute path of `path` inside the git directory, as `git rev-parse --git-path` resolves it.
    fn git_path(&self, path: &str) -> Option<PathBuf> {
        self.absolute_path(&["--git-path", path])
    }

    /// Returns the absolute path of the directory a linked worktree shares with the main worktree.
    fn common_dir(&self) -> Option<PathBuf> {
        self.absolute_path(&["--git-common-dir"])
    }

    /// Returns the path `git rev-parse` prints for `query`, made absolute.
    ///
    /// Queries with `--path-format=absolute` first. Git before 2.31 does not recognise the option, and the fallback
    /// joins the printed path onto the directory where the query ran.
    fn absolute_path(&self, query: &[&str]) -> Option<PathBuf> {
        let mut arguments = vec!["rev-parse", PATH_FORMAT];
        arguments.extend_from_slice(query);
        let printed = text(&self.directory, &arguments)?;
        let printed = if echoes_path_format(&printed) {
            let mut arguments = vec!["rev-parse"];
            arguments.extend_from_slice(query);
            text(&self.directory, &arguments)?
        } else {
            printed
        };
        Some(self.directory.join(printed))
    }
}

/// Option that has `git rev-parse` print absolute paths, from git 2.31 on.
const PATH_FORMAT: &str = "--path-format=absolute";

/// Returns whether `printed`, the output of a `git rev-parse` given [`PATH_FORMAT`], comes from a git before 2.31.
///
/// Such a git treats the option as a revision argument and echoes it, before a path relative to the directory where
/// the query ran.
pub(crate) fn echoes_path_format(printed: &str) -> bool {
    printed.lines().next() == Some(PATH_FORMAT)
}

/// Returns the files under the `src` of the crate in `directory` that git tracks or does not ignore, relative to
/// `src` with `/` separators.
pub(crate) fn list_sources(directory: &Path) -> Vec<String> {
    let arguments = [
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--exclude-standard",
        "--",
        "src",
    ];
    let Some(output) = run(directory, &arguments) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output)
        .split('\0')
        .filter_map(|path| path.strip_prefix("src/"))
        .map(str::to_owned)
        .collect()
}

/// Returns whether `text` has the form `YYYY-MM-DD`.
pub(crate) fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// Returns the standard output of git run with `arguments` in `directory`, `None` when it fails or git is absent.
fn run(directory: &Path, arguments: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .env("GIT_OPTIONAL_LOCKS", "0")
        // Variables set by a git hook or an outer git command would point every query at another repository.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

/// Returns the standard output of git run with `arguments` in `directory` as text, trimmed.
fn text(directory: &Path, arguments: &[&str]) -> Option<String> {
    let output = run(directory, arguments)?;
    Some(String::from_utf8_lossy(&output).trim().to_owned())
}
