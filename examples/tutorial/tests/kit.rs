//! The testing kit over the five tutorial models, as a reader's crate runs it, and their ids beside the example
//! models'.

use henad::testing::{CheckSettings, TestDeviceRequest, assert_set_conforms, headless_test_device};

#[test]
fn the_tutorial_models_conform() {
    let models = henad_tutorial::models().expect("every tutorial model has an id of its own");
    let mut settings = CheckSettings::default();
    if let Some(device) = headless_test_device(&TestDeviceRequest::baseline()) {
        settings = settings.gpu(device);
    } else {
        log::warn!("checking the tutorial models without their GPU checks: no adapter");
    }
    assert_set_conforms(&models, &settings);
}

#[test]
fn the_tutorial_models_join_the_example_set() {
    let tutorial = henad_tutorial::models().expect("every tutorial model has an id of its own");
    let mut models = henad::models::example_models();
    let examples = models.len();
    models
        .extend(tutorial)
        .expect("no tutorial model takes the id of an example model");
    assert_eq!(models.len(), examples + 5);
}
