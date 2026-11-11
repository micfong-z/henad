//! Test-only modules for the CPU engines and runner.

#[cfg(not(target_arch = "wasm32"))]
mod sim_thread_pacing;
mod sim_thread_snapshot;
#[cfg(not(target_arch = "wasm32"))]
mod sim_thread_stop;
pub(super) mod support;
