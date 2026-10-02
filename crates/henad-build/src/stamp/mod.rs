//! Build stamps: the commit a crate was built from, the commit's date, a dirty flag and a hash of the crate's sources.
//!
//! A stamp reads `.cargo_vcs_info.json` in a package that holds one, as every registry download does. Otherwise it
//! asks git, and only when git tracks the crate's own `Cargo.toml`. Outside both it records no commit, and the source
//! hash alone identifies the build.
//!
//! The source hash is a Fowler-Noll-Vo (FNV-1a) hash over the files under `src`, the manifest and, outside a package,
//! the nearest `Cargo.lock`. Dotfiles, editor backups and dangling symlinks stay out of it, and out of the dirty flag.

mod files;
mod git;

use std::path::{Path, PathBuf};

pub(crate) use files::{CrateFiles, VcsInfo, hash_sources};
pub(crate) use git::Repository;
#[cfg(test)]
pub(crate) use git::echoes_path_format;

use files::nearest_lockfile;

/// Packages whose sources the engine stamp covers in Henad's checkout.
const ENGINE_PACKAGES: [&str; 4] = ["henad-core", "henad-build", "henad-compute", "henad-explore"];
/// Directory of henad-explore below the root of Henad's repository, as `git rev-parse --show-prefix` prints it.
const ENGINE_PREFIX: &str = "crates/henad-explore/";

/// Crate a stamp is for. The scope sets the files the stamp covers and the sources of its commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StampScope {
    /// A host or model crate, through [`crate::stamp_commit`].
    Commit,
    /// henad-explore, whose stamp stands for the engine.
    Engine,
    /// henad-compute or henad-models: the crate's own `src` and manifest, and a commit only from a package.
    SourceHash,
}

/// Values one stamp sets, and the paths Cargo watches for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Stamp {
    /// Short commit hash, empty when the stamp could not learn it.
    pub(crate) commit: String,
    pub(crate) commit_date: String,
    pub(crate) dirty: Option<bool>,
    pub(crate) source_hash: Option<u64>,
    /// Hash of the crate's own `src` and manifest. Set for [`StampScope::Engine`] alone.
    pub(crate) crate_hash: Option<u64>,
    /// Paths whose change reruns the build script, each one that exists.
    pub(crate) watched: Vec<PathBuf>,
}

impl Stamp {
    /// Returns the stamp of the crate in `crate_dir` for `scope`.
    pub(crate) fn of(crate_dir: &Path, scope: StampScope) -> Self {
        let own = CrateFiles::new(crate_dir);
        if let Some(info) = VcsInfo::read(crate_dir) {
            return Self::packaged(&own, &info, scope);
        }
        let repository = Repository::find(crate_dir);
        match scope {
            StampScope::Commit => match &repository {
                Some(repository) => Self::from_git(repository, &[own]),
                None => Self::untracked(&own, None, true),
            },
            StampScope::Engine => {
                let mut stamp = match repository.as_ref().filter(|found| found.prefix() == ENGINE_PREFIX) {
                    Some(henad) => {
                        let packages: Vec<CrateFiles> = ENGINE_PACKAGES
                            .iter()
                            .map(|package| CrateFiles::in_repository(henad, package))
                            .collect();
                        Self::from_git(henad, &packages)
                    }
                    None => Self::untracked(&own, repository.as_ref(), true),
                };
                stamp.crate_hash = hash_sources(own.sources(repository.as_ref()));
                stamp
            }
            StampScope::SourceHash => Self::untracked(&own, repository.as_ref(), false),
        }
    }

    /// Returns the stamp of a package that holds a `.cargo_vcs_info.json`, hashed over its own files and no lockfile.
    fn packaged(own: &CrateFiles, info: &VcsInfo, scope: StampScope) -> Self {
        let source_hash = hash_sources(own.sources(None));
        Self {
            commit: info.short_commit().to_owned(),
            commit_date: String::new(),
            dirty: Some(info.dirty),
            source_hash,
            crate_hash: source_hash.filter(|_| scope == StampScope::Engine),
            watched: own.watched(),
        }
    }

    /// Returns the stamp of `packages` under git.
    fn from_git(repository: &Repository, packages: &[CrateFiles]) -> Self {
        let mut sources = Vec::new();
        let mut watched = repository.watched();
        let mut pathspecs = Vec::new();
        for package in packages {
            sources.extend(package.sources(Some(repository)));
            watched.extend(package.watched());
            pathspecs.extend(package.pathspecs(repository));
        }
        if let Some(lockfile) = nearest_lockfile(repository.directory()) {
            pathspecs.extend(repository.pathspec(&lockfile));
            sources.push(("Cargo.lock".to_owned(), lockfile.clone()));
            watched.push(lockfile);
        }
        let commit = repository.commit();
        let dirty = if commit.is_empty() {
            None
        } else {
            repository.dirty(&pathspecs)
        };
        Self {
            commit_date: repository.commit_date(),
            commit,
            dirty,
            source_hash: hash_sources(sources),
            crate_hash: None,
            watched,
        }
    }

    /// Returns the stamp of a crate whose commit stays unknown, hashed over its own files and, with `lockfile`, the
    /// nearest `Cargo.lock` when the crate is no package.
    ///
    /// `repository`, when given, lists the crate's files as git sees them, and watches none of its paths.
    fn untracked(own: &CrateFiles, repository: Option<&Repository>, lockfile: bool) -> Self {
        let mut sources = own.sources(repository);
        let mut watched = own.watched();
        if lockfile
            && !own.is_package()
            && let Some(lockfile) = nearest_lockfile(own.directory())
        {
            sources.push(("Cargo.lock".to_owned(), lockfile.clone()));
            watched.push(lockfile);
        }
        Self {
            commit: String::new(),
            commit_date: String::new(),
            dirty: None,
            source_hash: hash_sources(sources),
            crate_hash: None,
            watched,
        }
    }

    /// Returns the lines a build script prints to hand the stamp to Cargo.
    ///
    /// Each line takes the single-colon form. The double one needs Cargo 1.77 and would raise every downstream crate's
    /// MSRV.
    pub(crate) fn cargo_lines(&self, scope: StampScope) -> Vec<String> {
        let hex = |hash: Option<u64>| hash.map(|hash| format!("{hash:016x}")).unwrap_or_default();
        let dirty = self.dirty.map(|dirty| dirty.to_string()).unwrap_or_default();
        let mut lines = vec![
            format!("cargo:rustc-env=HENAD_BUILD_COMMIT={}", self.commit),
            format!("cargo:rustc-env=HENAD_BUILD_COMMIT_DATE={}", self.commit_date),
            format!("cargo:rustc-env=HENAD_BUILD_DIRTY={dirty}"),
            format!("cargo:rustc-env=HENAD_BUILD_SOURCE_HASH={}", hex(self.source_hash)),
        ];
        if scope == StampScope::Engine {
            lines.push(format!(
                "cargo:rustc-env=HENAD_BUILD_CRATE_HASH={}",
                hex(self.crate_hash)
            ));
            lines.push(format!(
                "cargo:rustc-env=HENAD_BUILD_STAMP_VERSION={}",
                env!("CARGO_PKG_VERSION")
            ));
        }
        lines.extend(
            self.watched
                .iter()
                .map(|path| format!("cargo:rerun-if-changed={}", path.display())),
        );
        lines
    }
}

/// Prints the stamp for `scope` of the crate whose build script runs.
#[expect(clippy::print_stdout, reason = "a build script talks to Cargo through stdout")]
pub(crate) fn print(scope: StampScope) {
    let Some(crate_dir) = std::env::var_os("CARGO_MANIFEST_DIR") else {
        println!("cargo:warning=`CARGO_MANIFEST_DIR` is not set. Call the stamp from a build script");
        return;
    };
    for line in Stamp::of(Path::new(&crate_dir), scope).cargo_lines(scope) {
        println!("{line}");
    }
}
