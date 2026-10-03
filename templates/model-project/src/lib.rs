//! Models of the my-model project.

mod gpu_vote;
mod vote;

henad::include_shaders!();

use henad::authoring::register_gpu_grid_model;
use henad::authoring::register_grid_model;

/// Returns every model this crate provides.
///
/// # Errors
///
/// Returns [`henad::ModelSetError`] when two models share an id or an id breaks the id grammar.
pub fn models() -> Result<henad::ModelSet, henad::ModelSetError> {
    let mut models = henad::ModelSet::new(henad::build_info!());
    models.insert(register_grid_model::<vote::Vote>())?;
    models.insert(register_gpu_grid_model::<gpu_vote::GpuVote>())?;
    Ok(models)
}

#[cfg(test)]
mod tests {
    use henad::testing::{CheckSettings, TestDeviceRequest, assert_set_conforms, headless_test_device};

    #[test]
    fn every_model_conforms() {
        let models = super::models().expect("model ids are unique");
        let mut settings = CheckSettings::default();
        if let Some(device) = headless_test_device(&TestDeviceRequest::baseline()) {
            settings = settings.gpu(device);
        }
        assert_set_conforms(&models, &settings);
    }
}
