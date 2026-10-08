//! Tests of the build stamps, over scratch crates in and out of git and a packaged henad-explore.

#![expect(clippy::print_stderr, reason = "a skipped test says why on stderr")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::support::{Scratch, read};
use crate::stamp::{CrateFiles, Repository, Stamp, StampScope, VcsInfo, echoes_path_format, hash_sources, is_date};

/// Returns whether git runs on this machine. Each test that needs it skips with a note otherwise.
fn has_git() -> bool {
    let present = Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    if !present {
        eprintln!("git is not installed, skipping");
    }
    present
}

/// Runs `git arguments` in `directory` with an identity and no signing, hooks or user configuration that could
/// change a commit, and returns its trimmed output.
fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=Henad Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "core.autocrlf=false",
        ])
        .args(arguments)
        .current_dir(directory)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Returns the first eight characters of the hash of `HEAD` in `directory`, as a stamp records it.
fn head(directory: &Path) -> String {
    git(directory, &["rev-parse", "HEAD"]).chars().take(8).collect()
}

/// Creates a repository in `directory` that keeps its refs as files, whatever the user's default format.
///
/// Packed refs exist only in that format. Git before 2.45 supports no other format, and rejects the option.
fn init(directory: &Path) {
    let with_format = Command::new("git")
        .args(["-c", "init.defaultBranch=main", "init", "--quiet", "--ref-format=files"])
        .current_dir(directory)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .output()
        .expect("git runs");
    if !with_format.status.success() {
        git(directory, &["init", "--quiet"]);
    }
}

/// Writes `contents` to `relative` below `directory`, creating its parents.
fn write(directory: &Path, relative: &str, contents: &str) {
    let path = directory.join(relative);
    std::fs::create_dir_all(path.parent().expect("a file has a parent")).expect("the directory can be created");
    std::fs::write(path, contents).expect("the file can be written");
}

/// Writes a crate named `package` into `directory`: a manifest and one source file.
fn write_crate(directory: &Path, package: &str) {
    write(
        directory,
        "Cargo.toml",
        &format!("[package]\nname = \"{package}\"\nversion = \"0.1.0\"\n"),
    );
    write(directory, "src/lib.rs", "pub fn step() {}\n");
}

/// Returns a git repository in a fresh scratch directory holding one crate and a lockfile, with one commit.
fn committed_crate(test: &str) -> Scratch {
    let scratch = Scratch::new(test);
    let directory = scratch.path();
    write_crate(directory, "model");
    write(directory, "Cargo.lock", "version = 4\n");
    init(directory);
    git(directory, &["add", "."]);
    git(directory, &["commit", "--quiet", "--no-verify", "-m", "first"]);
    scratch
}

/// Returns the contents of each of `paths`: a file's bytes, or every file below a directory with its bytes.
fn contents(paths: &[PathBuf]) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut pending = paths.to_vec();
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            let entries = std::fs::read_dir(&path).expect("the directory can be read");
            pending.extend(entries.map(|entry| entry.expect("an entry can be read").path()));
        } else if let Ok(bytes) = std::fs::read(&path) {
            found.insert(path, bytes);
        }
    }
    found
}

/// Returns the paths a stamp watches inside the git directory.
fn git_paths(stamp: &Stamp) -> Vec<PathBuf> {
    stamp
        .watched
        .iter()
        .filter(|path| path.components().any(|component| component.as_os_str() == ".git"))
        .cloned()
        .collect()
}

/// Commits a change to `README.md` in `directory`, and checks that a path that `stamp` watches in the git directory
/// changed with it and that a new stamp records the new commit.
fn assert_a_commit_changes_a_watched_path(stamp: &Stamp, directory: &Path, label: &str) {
    let watched = git_paths(stamp);
    assert!(!watched.is_empty(), "{label}: the stamp watches no git path");
    let before = contents(&watched);
    write(directory, "README.md", &format!("{label}\n"));
    git(directory, &["add", "README.md"]);
    git(directory, &["commit", "--quiet", "--no-verify", "-m", label]);
    assert_ne!(
        contents(&watched),
        before,
        "{label}: a commit changed none of {watched:?}"
    );
    let after = Stamp::of(directory, StampScope::Commit);
    assert_eq!(after.commit, head(directory), "{label}");
}

#[test]
fn a_commit_changes_a_watched_path_under_packed_refs_and_in_a_worktree() {
    if !has_git() {
        return;
    }
    let scratch = committed_crate("packed");
    let directory = scratch.path();
    git(directory, &["pack-refs", "--all"]);
    assert!(
        !directory.join(".git/refs/heads/main").exists(),
        "the branch ref is packed"
    );
    let stamp = Stamp::of(directory, StampScope::Commit);
    assert_eq!(stamp.commit, head(directory));
    assert_eq!(stamp.dirty, Some(false));
    assert_a_commit_changes_a_watched_path(&stamp, directory, "packed");

    let worktree = scratch.path().join("linked");
    git(
        directory,
        &[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "linked",
            worktree.to_str().expect("a UTF-8 path"),
        ],
    );
    let stamp = Stamp::of(&worktree, StampScope::Commit);
    assert_eq!(stamp.commit, head(&worktree));
    assert_eq!(stamp.dirty, Some(false));
    assert!(
        stamp.watched.iter().any(|path| path.ends_with("packed-refs")),
        "the worktree's stamp watches the shared packed-refs: {:?}",
        stamp.watched
    );
    assert_a_commit_changes_a_watched_path(&stamp, &worktree, "worktree");
}

#[test]
fn a_dotfile_under_src_changes_no_hash() {
    if !has_git() {
        return;
    }
    let scratch = committed_crate("dotfile");
    let directory = scratch.path();
    let clean = Stamp::of(directory, StampScope::Commit);

    write(directory, ".gitignore", "/target\n.cache\n");
    git(directory, &["add", ".gitignore"]);
    git(directory, &["commit", "--quiet", "--no-verify", "-m", "ignore"]);
    let committed = Stamp::of(directory, StampScope::Commit);
    assert_eq!(committed.dirty, Some(false));
    assert_eq!(
        committed.source_hash, clean.source_hash,
        "a file outside `src` changes no hash"
    );

    write(directory, "src/.cache", "ignored");
    write(directory, "src/.DS_Store", "untracked and not ignored");
    write(directory, "src/lib.rs.swp", "swap");
    write(directory, "src/lib.rs~", "backup");
    write(directory, "src/#lib.rs#", "autosave");
    write(directory, "src/lib.rs.orig", "merge backup");
    write(directory, "src/lib.rs.rej", "rejected hunk");
    let cluttered = Stamp::of(directory, StampScope::Commit);
    assert_eq!(cluttered.dirty, Some(false));
    assert_eq!(cluttered.source_hash, committed.source_hash);

    write(directory, "src/vote.rs", "pub fn vote() {}\n");
    let untracked = Stamp::of(directory, StampScope::Commit);
    assert_eq!(
        untracked.dirty,
        Some(true),
        "an untracked kernel file makes the tree dirty"
    );
    assert_ne!(untracked.source_hash, committed.source_hash);
}

#[cfg(unix)]
#[test]
fn a_dangling_or_directory_symlink_changes_neither_hash_nor_flag() {
    use std::os::unix::fs::symlink;

    if !has_git() {
        return;
    }
    let scratch = committed_crate("symlinks");
    let directory = scratch.path();
    let clean = Stamp::of(directory, StampScope::Commit);
    std::fs::create_dir_all(directory.join("assets")).expect("the directory can be created");
    symlink("missing.rs", directory.join("src/dangling.rs")).expect("a symlink can be made");
    symlink("../assets", directory.join("src/assets")).expect("a symlink can be made");
    let linked = Stamp::of(directory, StampScope::Commit);
    assert_eq!(linked.dirty, Some(false));
    assert_eq!(linked.source_hash, clean.source_hash);

    symlink("lib.rs", directory.join("src/alias.rs")).expect("a symlink can be made");
    let to_file = Stamp::of(directory, StampScope::Commit);
    assert_eq!(to_file.dirty, Some(true), "a symlink to a file counts as the file");
    assert_ne!(to_file.source_hash, clean.source_hash);
}

#[test]
fn an_uncommitted_edit_marks_the_tree_dirty_and_changes_the_hash() {
    if !has_git() {
        return;
    }
    let scratch = committed_crate("edit");
    let directory = scratch.path();
    let clean = Stamp::of(directory, StampScope::Commit);
    write(directory, "src/lib.rs", "pub fn step() { let _ = 1; }\n");
    let edited = Stamp::of(directory, StampScope::Commit);
    assert_eq!((clean.dirty, edited.dirty), (Some(false), Some(true)));
    assert_eq!(edited.commit, clean.commit);
    assert_ne!(edited.source_hash, clean.source_hash);

    let scratch = committed_crate("lockfile");
    let directory = scratch.path();
    let clean = Stamp::of(directory, StampScope::Commit);
    write(directory, "Cargo.lock", "version = 4\n# updated\n");
    let updated = Stamp::of(directory, StampScope::Commit);
    assert_eq!(updated.dirty, Some(true), "a lockfile change marks the tree dirty");
    assert_ne!(updated.source_hash, clean.source_hash);
}

#[test]
fn a_lockfile_git_does_not_track_leaves_the_flag_unknown() {
    if !has_git() {
        return;
    }
    let scratch = Scratch::new("ignored_lockfile");
    let directory = scratch.path();
    write_crate(directory, "model");
    write(directory, ".gitignore", "/target\nCargo.lock\n");
    init(directory);
    git(directory, &["add", "."]);
    git(directory, &["commit", "--quiet", "--no-verify", "-m", "first"]);
    write(directory, "Cargo.lock", "version = 4\n");
    let first = Stamp::of(directory, StampScope::Commit);
    assert_eq!(first.commit, head(directory));
    assert_eq!(first.dirty, None, "no commit records an ignored lockfile");
    write(directory, "Cargo.lock", "version = 4\n# updated\n");
    let updated = Stamp::of(directory, StampScope::Commit);
    assert_eq!(updated.dirty, None);
    assert_ne!(
        updated.source_hash, first.source_hash,
        "the hash tells the two locks apart"
    );
    write(directory, "src/lib.rs", "pub fn step() { let _ = 1; }\n");
    assert_eq!(
        Stamp::of(directory, StampScope::Commit).dirty,
        Some(true),
        "an edit under `src` still reads as one"
    );

    let repository = Repository::find(directory).expect("git tracks the crate");
    assert_eq!(repository.dirty(&[]), None, "an empty list of pathspecs tells nothing");
}

#[test]
fn a_crate_in_a_subdirectory_reads_its_pathspecs_from_git() {
    if !has_git() {
        return;
    }
    let scratch = Scratch::new("subdirectory");
    let root = scratch.path();
    let directory = root.join("models/vote");
    write_crate(&directory, "vote");
    write(root, "Cargo.lock", "version = 4\n");
    init(root);
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "--no-verify", "-m", "first"]);
    let clean = Stamp::of(&directory, StampScope::Commit);
    assert_eq!((clean.commit.clone(), clean.dirty), (head(root), Some(false)));
    write(root, "models/vote/src/lib.rs", "pub fn step() { let _ = 3; }\n");
    assert_eq!(Stamp::of(&directory, StampScope::Commit).dirty, Some(true));
    git(root, &["checkout", "--quiet", "--", "models/vote/src/lib.rs"]);
    write(root, "Cargo.lock", "version = 4\n# updated\n");
    assert_eq!(
        Stamp::of(&directory, StampScope::Commit).dirty,
        Some(true),
        "the workspace's lockfile above the crate is watched"
    );
}

#[test]
fn a_stray_original_manifest_leaves_the_crate_no_package() {
    let scratch = Scratch::new("stray_orig");
    let directory = scratch.path();
    write_crate(directory, "model");
    write(directory, "Cargo.lock", "version = 4\n");
    write(directory, "Cargo.toml.orig", "<<<<<<< ours\n");
    assert!(
        !CrateFiles::new(directory).is_package(),
        "a backup beside a hand-written manifest"
    );
    let stamp = Stamp::of(directory, StampScope::Commit);
    assert!(stamp.watched.iter().any(|path| path.ends_with("Cargo.lock")));
    write(
        directory,
        "Cargo.toml",
        "[package]\nname = \"model\"\nversion = \"0.1.1\"\n",
    );
    assert_ne!(
        Stamp::of(directory, StampScope::Commit).source_hash,
        stamp.source_hash,
        "an edit to the real manifest changes the hash"
    );

    write(
        directory,
        "Cargo.toml",
        "# THIS FILE IS AUTOMATICALLY GENERATED BY CARGO\n#\n[package]\nname = \"model\"\n",
    );
    assert!(CrateFiles::new(directory).is_package(), "a manifest Cargo generated");
}

#[test]
fn a_first_commit_reruns_a_stamp_made_before_it() {
    if !has_git() {
        return;
    }
    let scratch = Scratch::new("first_commit");
    let directory = scratch.path();
    write_crate(directory, "model");
    write(directory, "Cargo.lock", "version = 4\n");
    init(directory);
    let before = Stamp::of(directory, StampScope::Commit);
    assert_eq!(
        (before.commit.as_str(), before.dirty),
        ("", None),
        "git tracks nothing yet"
    );
    assert!(
        before.watched.iter().all(|path| path.exists()),
        "a missing path reruns every build"
    );

    let watched = git_paths(&before);
    assert!(
        !watched.is_empty(),
        "the stamp watches no git path: {:?}",
        before.watched
    );
    let contents_before = contents(&watched);
    git(directory, &["add", "."]);
    git(directory, &["commit", "--quiet", "--no-verify", "-m", "first"]);
    assert_ne!(
        contents(&watched),
        contents_before,
        "the first commit changed none of {watched:?}"
    );
    let after = Stamp::of(directory, StampScope::Commit);
    assert_eq!((after.commit, after.dirty), (head(directory), Some(false)));
}

#[test]
fn a_commit_date_reads_as_a_date_under_show_signature() {
    assert!(is_date("2026-10-03"));
    assert!(!is_date("No signature\n2026-10-03"));
    assert!(!is_date("gpg: Signature made"));
    if !has_git() {
        return;
    }
    let scratch = committed_crate("signature");
    let directory = scratch.path();
    let key = directory.join("key");
    let keygen = Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-N", "", "-q", "-f"])
        .arg(&key)
        .status();
    if !keygen.is_ok_and(|status| status.success()) {
        eprintln!("ssh-keygen is not installed, skipping the signed commit");
        return;
    }
    let signing_key = format!("user.signingkey={}", key.display());
    write(directory, "README.md", "signed\n");
    git(directory, &["add", "README.md"]);
    git(
        directory,
        &[
            "-c",
            "gpg.format=ssh",
            "-c",
            signing_key.as_str(),
            "commit",
            "--quiet",
            "--no-verify",
            "-S",
            "-m",
            "signed",
        ],
    );
    git(directory, &["config", "log.showSignature", "true"]);
    let shown = git(directory, &["log", "-1", "--format=%cd", "--date=short"]);
    assert!(
        !is_date(&shown),
        "git prints the signature check ahead of the date: {shown:?}"
    );
    let stamp = Stamp::of(directory, StampScope::Commit);
    assert_eq!(
        stamp.commit_date,
        git(
            directory,
            &["log", "-1", "--no-show-signature", "--format=%cd", "--date=short"]
        )
    );
    assert!(is_date(&stamp.commit_date), "{:?}", stamp.commit_date);
}

#[test]
fn a_crate_outside_git_records_its_hash_alone() {
    let scratch = Scratch::new("untracked");
    let directory = scratch.path();
    write_crate(directory, "model");
    write(directory, "Cargo.lock", "version = 4\n");
    let stamp = Stamp::of(directory, StampScope::Commit);
    assert_eq!((stamp.commit.as_str(), stamp.dirty), ("", None));
    assert!(stamp.source_hash.is_some());
    assert!(stamp.watched.iter().any(|path| path.ends_with("src")));
    assert!(stamp.watched.iter().any(|path| path.ends_with("Cargo.lock")));

    write(directory, "src/.DS_Store", "finder");
    assert_eq!(Stamp::of(directory, StampScope::Commit).source_hash, stamp.source_hash);
    write(directory, "Cargo.lock", "version = 4\n# updated\n");
    assert_ne!(
        Stamp::of(directory, StampScope::Commit).source_hash,
        stamp.source_hash,
        "a cargo update changes the hash"
    );
    let source_hash = Stamp::of(directory, StampScope::SourceHash);
    assert_eq!(
        source_hash.source_hash,
        hash_sources(CrateFiles::new(directory).sources(None))
    );
    assert!(!source_hash.watched.iter().any(|path| path.ends_with("Cargo.lock")));
}

#[test]
fn a_crlf_checkout_hashes_like_lf() {
    let lf = Scratch::new("lf");
    let crlf = Scratch::new("crlf");
    write_crate(lf.path(), "model");
    write_crate(crlf.path(), "model");
    write(lf.path(), "src/step.rs", "fn a() {}\nfn b() {}\n");
    write(crlf.path(), "src/step.rs", "fn a() {}\r\nfn b() {}\r\n");
    let lf_manifest = read(&lf.path().join("Cargo.toml"));
    write(crlf.path(), "Cargo.toml", &lf_manifest.replace('\n', "\r\n"));
    let hash = |scratch: &Scratch| Stamp::of(scratch.path(), StampScope::SourceHash).source_hash;
    assert_eq!(hash(&lf), hash(&crlf));
    write(crlf.path(), "src/step.rs", "fn a() {}\r\nfn c() {}\r\n");
    assert_ne!(hash(&lf), hash(&crlf));
}

#[test]
fn the_engine_stamp_covers_its_siblings_only_in_henads_tree() {
    if !has_git() {
        return;
    }
    let scratch = Scratch::new("engine");
    let root = scratch.path();
    for package in ["henad-core", "henad-build", "henad-compute", "henad-explore"] {
        write_crate(&root.join("crates").join(package), package);
    }
    write(root, "Cargo.lock", "version = 4\n");
    init(root);
    git(root, &["add", "."]);
    git(root, &["commit", "--quiet", "--no-verify", "-m", "first"]);
    let explore = root.join("crates/henad-explore");

    let clean = Stamp::of(&explore, StampScope::Engine);
    assert_eq!(clean.commit, head(root));
    assert_eq!(clean.dirty, Some(false));
    let own = hash_sources(CrateFiles::new(&explore).sources(Repository::find(&explore).as_ref()));
    assert_eq!(clean.crate_hash, own);
    assert_ne!(clean.source_hash, own, "the engine hash covers the siblings");
    assert!(
        clean.watched.iter().any(|path| path.ends_with("crates/henad-core/src")),
        "{:?}",
        clean.watched
    );

    write(root, "crates/henad-core/src/lib.rs", "pub fn step() { let _ = 2; }\n");
    let edited = Stamp::of(&explore, StampScope::Engine);
    assert_eq!(edited.dirty, Some(true), "an edit to henad-core marks the engine dirty");
    assert_ne!(edited.source_hash, clean.source_hash);
    assert_eq!(edited.crate_hash, clean.crate_hash);

    let copied = root.join("vendor/henad-explore");
    write_crate(&copied, "henad-explore");
    git(root, &["add", "vendor"]);
    let elsewhere = Stamp::of(&copied, StampScope::Engine);
    assert_eq!(
        (elsewhere.commit.as_str(), elsewhere.dirty),
        ("", None),
        "a copy in another repository records no commit"
    );
    assert!(elsewhere.crate_hash.is_some());
}

#[test]
fn a_source_hash_stamp_reads_no_git_commit() {
    if !has_git() {
        return;
    }
    let scratch = committed_crate("source-hash");
    let stamp = Stamp::of(scratch.path(), StampScope::SourceHash);
    assert_eq!((stamp.commit.as_str(), stamp.dirty), ("", None));
    assert!(stamp.source_hash.is_some());
    assert!(
        stamp
            .watched
            .iter()
            .all(|path| !path.components().any(|component| component.as_os_str() == ".git")),
        "{:?}",
        stamp.watched
    );
    let lines = stamp.cargo_lines(StampScope::SourceHash);
    assert!(lines.contains(&"cargo:rustc-env=HENAD_BUILD_DIRTY=".to_owned()));
    assert!(!lines.iter().any(|line| line.contains("CRATE_HASH")));
}

#[test]
fn a_git_before_2_31_is_told_apart_by_its_echo() {
    assert!(echoes_path_format("--path-format=absolute\n.git/HEAD"));
    assert!(!echoes_path_format("/work/model/.git/HEAD"));
}

#[test]
fn vcs_info_reads_the_commit_and_the_dirty_flag() {
    let clean = "{\n  \"git\": {\n    \"sha1\": \"0123456789abcdef0123456789abcdef01234567\"\n  },\n  \"path_in_vcs\": \
                 \"crates/henad-explore\"\n}";
    let info = VcsInfo::parse(clean).expect("the file names a commit");
    assert_eq!((info.short_commit(), info.dirty), ("01234567", false));
    let dirty = clean.replace("\"\n  },", "\",\n    \"dirty\": true\n  },");
    assert!(VcsInfo::parse(&dirty).expect("the file names a commit").dirty);
    assert_eq!(VcsInfo::parse("{\"path_in_vcs\": \"\"}"), None);
}

/// Returns the root of Henad's workspace, `None` outside a checkout.
fn workspace_root() -> Option<PathBuf> {
    let output = Command::new(env!("CARGO"))
        .args(["locate-project", "--workspace", "--message-format", "plain"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .ok()?;
    let manifest = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let root = manifest.parent()?.to_path_buf();
    root.join("crates/henad-explore/Cargo.toml").is_file().then_some(root)
}

#[test]
fn the_engine_stamp_reads_cargo_vcs_info() {
    if !has_git() {
        return;
    }
    let Some(root) = workspace_root().filter(|root| Repository::find(&root.join("crates/henad-explore")).is_some())
    else {
        eprintln!("not in Henad's git checkout, skipping");
        return;
    };
    let scratch = Scratch::new("package");
    let target = scratch.path().join("target");
    let output = Command::new(env!("CARGO"))
        // henad-explore's siblings are packaged beside it, since Cargo resolves a lone package's path dependencies
        // from the registry.
        .args([
            "package",
            "-p",
            "henad-core",
            "-p",
            "henad-build",
            "-p",
            "henad-compute",
            "-p",
            "henad-explore",
            "-p",
            "henad-models",
            "--no-verify",
            "--allow-dirty",
            "--offline",
            "--target-dir",
        ])
        .arg(&target)
        .current_dir(&root)
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "cargo package failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Returns the directory the tarball of `name` unpacks to.
    let unpack = |name: &str| {
        let tarball = std::fs::read_dir(target.join("package"))
            .expect("cargo package writes its output")
            .map(|entry| entry.expect("an entry can be read").path())
            .find(|path| {
                path.file_name()
                    .is_some_and(|file| file.to_string_lossy().starts_with(&format!("{name}-")))
                    && path.extension().is_some_and(|extension| extension == "crate")
            })
            .expect("a tarball");
        let unpacked = scratch.path().join("unpacked").join(name);
        std::fs::create_dir_all(&unpacked).expect("the directory can be created");
        // The names are relative, and tar runs from the scratch directory. Windows' tar reads the colon of `C:` as a
        // remote host.
        let relative_tarball = tarball
            .strip_prefix(scratch.path())
            .expect("the tarball is in the scratch directory");
        let status = Command::new("tar")
            .arg("-xzf")
            .arg(relative_tarball)
            .arg("-C")
            .arg(Path::new("unpacked").join(name))
            .current_dir(scratch.path())
            .status()
            .expect("tar runs");
        assert!(status.success(), "tar unpacks the tarball");
        std::fs::read_dir(&unpacked)
            .expect("tar unpacks a directory")
            .next()
            .expect("one directory")
            .expect("an entry can be read")
            .path()
    };
    let package = unpack("henad-explore");

    let info = VcsInfo::read(&package).expect("the package holds a .cargo_vcs_info.json");
    let stamp = Stamp::of(&package, StampScope::Engine);
    assert_eq!(stamp.commit, head(&root));
    assert_eq!(stamp.commit, info.short_commit());
    assert_eq!(stamp.dirty, Some(info.dirty));
    assert_eq!(stamp.commit_date, "");
    assert_eq!(
        stamp.crate_hash, stamp.source_hash,
        "a package's engine hash covers henad-explore alone"
    );
    assert!(
        !stamp.watched.iter().any(|path| path.ends_with("Cargo.lock")),
        "a package watches no lockfile"
    );

    let checkout = Stamp::of(&root.join("crates/henad-explore"), StampScope::Engine);
    assert_eq!(
        stamp.crate_hash, checkout.crate_hash,
        "a package and the checkout it came from hash henad-explore alike"
    );

    // henad-compute's hash goes into every engine build, and henad-models' hash goes into the build of the example
    // models.
    for name in ["henad-compute", "henad-models"] {
        let packaged = Stamp::of(&unpack(name), StampScope::SourceHash);
        let checkout = Stamp::of(&root.join("crates").join(name), StampScope::SourceHash);
        assert!(packaged.source_hash.is_some(), "{name}");
        assert_eq!(
            packaged.source_hash, checkout.source_hash,
            "a package and the checkout it came from hash {name} alike"
        );
    }
}
