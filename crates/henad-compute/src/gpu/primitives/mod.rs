//! Building blocks the GPU engines and models share.
//!
//! The GPU counterparts of `henad_core`'s data structures live here rather than there, since
//! `henad-core` never sees a wgpu type.

pub mod dispatch;
pub mod pipeline;
pub mod prefix_scan;
pub mod readback;
pub mod reduce;
pub mod spatial_hash;
pub mod wgsl;
