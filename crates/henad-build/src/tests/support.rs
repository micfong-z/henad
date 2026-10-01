//! A temporary directory per test, holding a shader root and an `OUT_DIR`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// A shader that imports a shared module and binds one buffer.
pub(crate) const STEP_SHADER: &str = "\
#import henad::dispatch::{WORKGROUP, linear_index}

@group(0) @binding(0) var<storage, read_write> state: array<u32>;

@compute
@workgroup_size(WORKGROUP)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    state[linear_index(lid, wid, 1u)] = 1u;
}
";

/// A directory under the system's temporary directory, removed when dropped.
pub(crate) struct Scratch {
    path: PathBuf,
}

impl Scratch {
    /// Returns an empty directory named after `test`.
    pub(crate) fn new(test: &str) -> Self {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let path = std::env::temp_dir().join(format!(
            "henad-build-{test}-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        remove(&path);
        std::fs::create_dir_all(&path).expect("the scratch directory can be created");
        Self { path }
    }

    /// Shader root of the scratch crate.
    pub(crate) fn root(&self) -> PathBuf {
        self.path.join("src")
    }

    /// `OUT_DIR` of the scratch crate.
    pub(crate) fn out_dir(&self) -> PathBuf {
        self.path.join("out")
    }

    /// Writes `contents` to `relative`, a path below the shader root.
    pub(crate) fn write(&self, relative: &str, contents: &str) {
        let path = self.root().join(relative);
        std::fs::create_dir_all(path.parent().expect("a file has a parent")).expect("the directory can be created");
        std::fs::write(path, contents).expect("the file can be written");
    }

    /// Returns the text of the generated file `name`.
    pub(crate) fn generated(&self, name: &str) -> String {
        read(&self.out_dir().join(name))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        remove(&self.path);
    }
}

/// Removes the directory at `path` and everything in it, if it exists.
fn remove(path: &Path) {
    if path.exists() {
        std::fs::remove_dir_all(path).expect("the scratch directory can be removed");
    }
}

/// Returns the text of `path`.
pub(crate) fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}
