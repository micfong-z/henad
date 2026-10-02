//! Parameter sweeps and searches over Henad models.

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
pub mod pumped;
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
