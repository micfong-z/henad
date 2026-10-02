//! The testing kit over the five tutorial models, as a reader's crate runs it.

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
