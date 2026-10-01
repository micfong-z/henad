//! The shared WGSL modules a model shader reaches with `#import henad::<module>`, as text.
//!
//! Each module is the twin of a Rust primitive: `henad::dispatch` of the engine's linear dispatch, `henad::rng` of
//! [`super::rng`], and `henad::space` of [`super::space`]. `henad::dims` holds the uniform the grid engine writes for
//! a display or reduce shader, and `henad::reduce_tree` the workgroup sum a reduce shader repeats.
//!
//! henad-build writes these modules beside a crate's shaders before it composes them. The text here is the one source
//! every build and every host reads.

use crate::explore::fingerprint::Fnv1a64;

/// A WGSL module a model shader can `#import`, as text.
#[derive(Debug, Clone, Copy)]
pub struct SharedModule {
    /// Path a shader imports, as in `henad::rng`.
    pub import_path: &'static str,
    pub source: &'static str,
}

/// `henad::dispatch`, `henad::dims`, `henad::rng`, `henad::space` and `henad::reduce_tree`.
pub const SHARED_WGSL_MODULES: &[SharedModule] = &[
    SharedModule {
        import_path: "henad::dispatch",
        source: include_str!("dispatch.wgsl"),
    },
    SharedModule {
        import_path: "henad::dims",
        source: include_str!("dims.wgsl"),
    },
    SharedModule {
        import_path: "henad::rng",
        source: include_str!("rng.wgsl"),
    },
    SharedModule {
        import_path: "henad::space",
        source: include_str!("space.wgsl"),
    },
    SharedModule {
        import_path: "henad::reduce_tree",
        source: include_str!("reduce_tree.wgsl"),
    },
];

/// FNV-1a hash of every module's import path and source, in [`SHARED_WGSL_MODULES`] order.
pub const SHARED_WGSL_FNV1A64: u64 = {
    let mut hasher = Fnv1a64::new();
    let mut index = 0;
    while index < SHARED_WGSL_MODULES.len() {
        hasher.write_str(SHARED_WGSL_MODULES[index].import_path);
        hasher.write_str(SHARED_WGSL_MODULES[index].source);
        index += 1;
    }
    hasher.finish()
};

#[cfg(test)]
mod tests {
    use super::{SHARED_WGSL_FNV1A64, SHARED_WGSL_MODULES};
    use crate::explore::fingerprint::Fnv1a64;

    #[test]
    fn each_module_declares_its_import_path() {
        for module in SHARED_WGSL_MODULES {
            let first = module.source.lines().next().unwrap_or_default();
            assert_eq!(first, format!("#define_import_path {}", module.import_path));
        }
    }

    #[test]
    fn the_const_hash_matches_one_computed_at_run_time() {
        let mut hasher = Fnv1a64::new();
        for module in SHARED_WGSL_MODULES {
            hasher.write_str(module.import_path);
            hasher.write_str(module.source);
        }
        assert_eq!(SHARED_WGSL_FNV1A64, hasher.finish());
    }
}
