//! Git queries of a stamp: the commit, its date, the dirty flag, the files under `src` and the paths a commit
//! changes.
//!
//! Every query runs with `GIT_OPTIONAL_LOCKS=0`. A stamp then never takes `.git/index.lock` to refresh the index, and
//! a build never blocks a git command the user runs beside it.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::files::{is_excluded, is_link_to_no_file};

/// Repository that tracks a crate's `Cargo.toml`, seen from the crate's directory.
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
    /// Note that a package unpacked inside some unrelated repository, as under a `CARGO_HOME` inside one, is then
    /// outside git. Otherwise it would record that repository's commit.
    pub(crate) fn find(directory: &Path) -> Option<Self> {
        run(directory, &["ls-files", "--error-unmatch", "Cargo.toml"])?;
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
    /// The full hash is read and cut. `--short=8` lengthens a hash that eight characters leave ambiguous.
    pub(crate) fn commit(&self) -> String {
        let full = text(&self.directory, &["rev-parse", "HEAD"]).unwrap_or_default();
        full.chars().take(8).collect()
    }

    /// Returns the date of `HEAD`'s commit as `YYYY-MM-DD`.
    pub(crate) fn commit_date(&self) -> String {
        text(&self.directory, &["log", "-1", "--format=%cd", "--date=short"]).unwrap_or_default()
    }

    /// Returns `path` relative to the repository root with `/` separators, `None` for a path outside it.
    pub(crate) fn pathspec(&self, path: &Path) -> Option<String> {
        let path = absolute_real(path);
        let relative = path.strip_prefix(absolute_real(&self.toplevel)).ok()?;
        let components: Vec<String> = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect();
        Some(components.join("/"))
    }

    /// Returns whether any path below `pathspecs` differs from `HEAD`, untracked files that git does not ignore
    /// included. `None` when git cannot tell.
    ///
    /// Each path is checked against the exclusions below the pathspec it falls under, and a symlink to a directory
    /// or to nothing is left out, as the hash leaves it out. An untracked `src/.DS_Store` or `src/step.rs.swp` leaves
    /// the tree clean.
    pub(crate) fn dirty(&self, pathspecs: &[String]) -> Option<bool> {
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

    /// Returns the paths of the repository that change on every commit, reset and checkout, each one that exists.
    ///
    /// Each path comes from `git rev-parse --git-path` or `--git-common-dir`. Both resolve a linked worktree's own
    /// files and the files it shares. `HEAD` and `logs/HEAD` are the worktree's own. The branch's loose ref and
    /// `packed-refs` sit in the common directory, as does `refs/heads`, watched in place of `logs/HEAD` when
    /// `core.logAllRefUpdates` is off. A repository in the reftable format has `reftable` in place of the refs.
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

    /// Returns the absolute path of the directory a linked worktree shares with the main one.
    fn common_dir(&self) -> Option<PathBuf> {
        self.absolute_path(&["--git-common-dir"])
    }

    /// Returns the path `git rev-parse` prints for `query`, made absolute.
    ///
    /// Asks with `--path-format=absolute` first. Git before 2.31 does not know the option, and the second form joins
    /// the path it prints onto the directory the query ran in.
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
/// Such a git takes the option for a revision argument and prints it back, ahead of a path relative to the directory
/// the query ran in.
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

/// Returns `path` with every symlink resolved, or `path` itself when it cannot be resolved.
///
/// Git prints real paths, and Cargo's `CARGO_MANIFEST_DIR` can name the same directory through a symlink.
fn absolute_real(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Returns the standard output of git run with `arguments` in `directory`, `None` when it fails or git is absent.
fn run(directory: &Path, arguments: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .env("GIT_OPTIONAL_LOCKS", "0")
        // A variable a git hook or an outer git command set would point every query at another repository.
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
