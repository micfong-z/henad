//! Carries a paint callback's wgpu handles past `CallbackTrait`'s `Send + Sync` bound.
//!
//! egui builds a callback and paints it on the same thread. Under atomics a wgpu handle is neither
//! `Send` nor `Sync`, and `egui_wgpu::CallbackTrait` requires both on every target, so in that
//! build the handles are wrapped. `SendWrapper` panics as soon as a second thread touches them.
//!
//! Reach the payload by field access only. Without atomics, [`Painted<T>`] is `T` itself, so an
//! explicit `*` compiles on one target and not the other.

/// A paint payload, wrapped where wgpu handles are not `Send`.
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
pub type Painted<T> = send_wrapper::SendWrapper<T>;

/// A paint payload, wrapped where wgpu handles are not `Send`.
#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
pub type Painted<T> = T;

#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
pub fn painted<T>(value: T) -> Painted<T> {
    send_wrapper::SendWrapper::new(value)
}

#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
pub fn painted<T>(value: T) -> Painted<T> {
    value
}
