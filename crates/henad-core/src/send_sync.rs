//! `Send` and `Sync` bounds that relax on wasm with atomics.
//!
//! Named after `wgpu_types::WasmNotSend`, which exists for the same reason. A wgpu handle is
//! `Send + Sync` on wasm only through `fragile-send-sync-non-atomic-wasm`, and that feature turns
//! itself off once atomics are on. Anything holding a device or a queue stops being `Send` in the
//! threaded web build.
//!
//! Nothing is lost. One thread in a browser touches wgpu, and nothing is sent anywhere.

/// `Send`, relaxed to nothing on wasm with atomics.
#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
pub trait WasmNotSend: Send {}
#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
impl<T: Send> WasmNotSend for T {}

/// `Sync`, relaxed to nothing on wasm with atomics.
#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
pub trait WasmNotSync: Sync {}
#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
impl<T: Sync> WasmNotSync for T {}

#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
pub trait WasmNotSend {}
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
impl<T> WasmNotSend for T {}

#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
pub trait WasmNotSync {}
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
impl<T> WasmNotSync for T {}
