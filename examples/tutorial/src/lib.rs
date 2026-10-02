//! The finished code from `docs/guide/first-model/`, compiled and checked against the models it
//! teaches.
//!
//! The tutorials are written out by hand rather than included from the shipped models, so that a
//! page can show a half-finished function and grow it. That leaves the pages free to drift, which
//! is what these modules are here to stop. Each one is the state a reader reaches at the end of a
//! page, and `tests/parity.rs` steps it beside the model it mirrors and demands the same bits.
//!
//! Change a shipped model and one of two things happens. The parity test fails, and the page needs
//! the same edit. Or the authoring API moved and this stops compiling, which says the same thing
//! louder.
//!
//! The crate depends on the `henad` facade alone and names every item through it, as a reader's
//! crate does. The two GPU pages carry their own copies of the shipped shaders, and
//! `tests/shaders.rs` holds each copy to its original.

henad::include_shaders!();

pub mod foraging;
pub mod gpu_foraging;
pub mod gpu_life;
pub mod life;
pub mod virus;

use henad::authoring::{
    register_agent_model, register_gpu_agent_model, register_gpu_grid_model, register_grid_model,
    register_network_model,
};

/// Returns the five tutorial models, with this crate's build as their source.
///
/// # Errors
///
/// Returns [`henad::ModelSetError`] when two models share an id or an id breaks the id grammar.
pub fn models() -> Result<henad::ModelSet, henad::ModelSetError> {
    let mut models = henad::ModelSet::new(henad::build_info!());
    models
        .insert(register_grid_model::<life::LifeModel>())?
        .insert(register_agent_model::<foraging::ForagingModel>())?
        .insert(register_network_model::<virus::VirusModel>())?
        .insert(register_gpu_grid_model::<gpu_life::GpuLifeModel>())?
        .insert(register_gpu_agent_model::<gpu_foraging::GpuForagingModel>())?;
    Ok(models)
}
