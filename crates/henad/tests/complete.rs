//! Runs the program of `examples/complete.rs` up to the app, and checks that it prints what `docs/guide/library.md`
//! shows for it.
//!
//! The program comes in whole. Its `main` opens a window, and the test calls `study` alone, on a folder of its own.

#![expect(clippy::print_stderr, reason = "a skipped comparison says why on stderr")]

include!("../examples/complete.rs");

/// Returns the lines of the first `text` block after the line "It prints:" in `guide`.
///
/// # Panics
///
/// Panics when the guide holds no such block.
fn printed_in_guide(guide: &str) -> Vec<&str> {
    let mut lines = guide.lines().skip_while(|line| line.trim() != "It prints:");
    assert!(
        lines.any(|line| line.trim() == "```text"),
        "the guide shows the output after \"It prints:\""
    );
    lines.take_while(|line| line.trim() != "```").collect()
}

/// Returns whether `printed` matches `shown`, a line `...` in `shown` standing for any number of lines.
fn matches_shown(printed: &[&str], shown: &[&str]) -> bool {
    let Some(gap) = shown.iter().position(|line| *line == "...") else {
        return printed == shown;
    };
    let (head, rest) = (&shown[..gap], &shown[gap + 1..]);
    printed.starts_with(head) && (head.len()..=printed.len()).any(|start| matches_shown(&printed[start..], rest))
}

#[test]
fn the_complete_program_prints_what_the_guide_shows() {
    let models = henad::models::example_models();
    // A folder of this process's own. A test running in another process at the same time never touches it.
    let folder = std::env::temp_dir().join(format!("henad-complete-{}", std::process::id()));
    if folder.exists() {
        std::fs::remove_dir_all(&folder).expect("a folder left by an earlier process is removed");
    }
    let mut out = Vec::new();
    let studied = study(&models, &folder, &mut out);
    let removed = std::fs::remove_dir_all(&folder);
    let replay = studied.expect("the study runs");
    removed.expect("the sweep's folder is removed");
    assert_eq!(replay.model, "sir");
    let printed = String::from_utf8(out).expect("the program writes UTF-8");
    let printed: Vec<&str> = printed.lines().collect();

    // Read at run time. The guide sits outside the package, and a crate built from its tarball skips the comparison.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/guide/library.md");
    let Ok(guide) = std::fs::read_to_string(&path) else {
        eprintln!("note: skipped the comparison, {} is absent", path.display());
        return;
    };
    let shown = printed_in_guide(&guide);
    assert!(
        matches_shown(&printed, &shown),
        "the program printed\n{}\nand the guide shows\n{}",
        printed.join("\n"),
        shown.join("\n")
    );
}
