//! The finished code from `docs/guide/first-model/`, compiled and checked against the models it
//! teaches.
//!
//! The tutorials are written out by hand rather than included from the shipped models, so that a
//! page can show a half-finished function and grow it. That leaves the pages free to drift, which
//! is what these modules are here to stop. Each one is the state a reader reaches at the end of a
//! page, and `parity.rs` steps it beside the model it mirrors and demands the same bits.
//!
//! Change a shipped model and one of two things happens. The parity test fails, and the page needs
//! the same edit. Or the authoring API moved and this stops compiling, which says the same thing
//! louder.
//!
//! The two GPU pages bind the shipped shaders rather than carrying copies, since a shader holds no
//! model id and the page writes it out unchanged. Their twins hold the Rust half only.

pub mod foraging;
pub mod gpu_foraging;
pub mod gpu_life;
pub mod life;

mod parity;
