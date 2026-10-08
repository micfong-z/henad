//! Primitives a model kernel calls, most of them paired with a WGSL twin under `henad::`.
//!
//! `space::offsets`, `space::for_each_neighbor`, `rng::xorshift64`, `rng::mix_seed` and `rng::next_index` are Rust
//! only, and `henad::space` adds `neighbor_count` and `neighbor_offset` in WGSL alone. The
//! [primitives reference](https://micfong-z.github.io/henad/reference/primitives/) indexes them and lists the
//! deliberate omissions.

pub mod rng;
pub mod space;
pub mod wgsl;
