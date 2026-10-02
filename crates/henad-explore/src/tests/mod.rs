//! Test-only modules.

pub mod broken;
#[cfg(not(target_arch = "wasm32"))]
pub mod support;

#[cfg(not(target_arch = "wasm32"))]
mod determinism;
#[cfg(not(target_arch = "wasm32"))]
mod failure;
#[cfg(not(target_arch = "wasm32"))]
mod gpu;
#[cfg(not(target_arch = "wasm32"))]
mod handle;
#[cfg(not(target_arch = "wasm32"))]
mod kit;
#[cfg(not(target_arch = "wasm32"))]
mod provenance;
#[cfg(not(target_arch = "wasm32"))]
mod replay;
#[cfg(not(target_arch = "wasm32"))]
mod result_set;
#[cfg(not(target_arch = "wasm32"))]
mod resume;
#[cfg(not(target_arch = "wasm32"))]
mod run_control;
#[cfg(not(target_arch = "wasm32"))]
mod search;
#[cfg(not(target_arch = "wasm32"))]
mod shards;
#[cfg(not(target_arch = "wasm32"))]
mod tracks;
