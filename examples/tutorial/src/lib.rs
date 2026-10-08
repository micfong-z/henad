//! The finished code of the five pages under `docs/guide/first-model/`, one module per page.
//!
//! Each module holds the state a reader reaches at the end of its page. `tests/parity.rs` steps it beside the
//! example model it teaches and demands the same bits, `tests/snippets.rs` checks the code each page shows against
//! it, and `tests/shaders.rs` checks the GPU pages' shader copies against the shipped shaders. A behaviour change in
//! an example model that a page teaches fails a parity test until the page and its module follow, and a breaking
//! change to the authoring API stops this crate compiling.
//!
//! The crate depends on the `henad` facade alone and refers to every item by its facade path, as a reader's crate does.

// Proving a type that holds wgpu handles `Send` or `Sync` walks wgpu-core's registries, deeper than the default
// limit of 128.
#![recursion_limit = "256"]

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
