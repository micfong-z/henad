//! Parameter sweeps and searches over Henad models, and the folders of results they write.
//!
//! A sweep plans a spec against a model and runs every planned run. A search runs a spec's `[search]` table a batch of
//! candidates at a time. On native, `sweep::run_spec` runs either and blocks until it ends, and `sweep::plan_spec`
//! plans and probes one without running it. [`handle::SweepRun`] runs one beside a host's frame loop, on its own
//! thread on native and pumped from the frames in a browser. [`spec_file`] reads and writes the TOML form of a spec.
//!
//! A sweep writes `manifest.json`, `runs.csv`, `series.csv` and `summary.csv` to a folder or to memory, and a search
//! writes its own tables beside them. [`merge::merge`] joins the folders of a sweep's shards, and
//! [`result_set::ResultSet`] reads a folder back. `benchmark` times a model's step loop, and `testing` holds the
//! checks that a model's tests run against its entry.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]
// Proving a type that holds wgpu handles `Send` or `Sync` walks wgpu-core's registries, deeper than the default
// limit of 128.
#![recursion_limit = "256"]

#[cfg(not(target_arch = "wasm32"))]
pub mod benchmark;
pub mod cursor;
#[cfg(not(target_arch = "wasm32"))]
pub mod device;
pub mod exec;
pub mod handle;
pub mod merge;
pub mod output;
pub mod probe;
pub mod progress;
#[cfg(any(target_arch = "wasm32", test))]
pub(crate) mod pumped;
pub mod result_set;
pub mod schema;
pub mod search_run;
pub mod spec_file;
pub mod sweep;
#[cfg(any(feature = "testing", test))]
pub mod testing;

#[cfg(test)]
mod tests;

use henad_core::provenance::BuildInfo;

/// Henad's own build, stamped by henad-explore's build script over the engine's crates.
///
/// In Henad's checkout the source hash covers henad-core, henad-build, henad-compute and henad-explore with the
/// workspace's lockfile. In a package it covers henad-explore alone.
pub const ENGINE_BUILD: BuildInfo = henad_core::build_info!();

/// Hash of henad-explore's own `src` and manifest as 16 hexadecimal digits, empty without a stamp.
pub(crate) const CRATE_HASH: &str = match option_env!("HENAD_BUILD_CRATE_HASH") {
    Some(hash) => hash,
    None => "",
};

/// Version of the henad-build that stamped henad-explore, empty without a stamp.
pub(crate) const STAMP_VERSION: &str = match option_env!("HENAD_BUILD_STAMP_VERSION") {
    Some(version) => version,
    None => "",
};
