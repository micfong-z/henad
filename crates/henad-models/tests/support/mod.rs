//! Helpers the consistency tests share: the directory and the files of the reference fixtures, a network model's
//! parameters by id, and the binomial acceptance band.

#![allow(dead_code, reason = "each test crate uses its own part of the module")]

use std::path::{Path, PathBuf};

use henad_compute::cpu::network_engine::network_model_param_descriptors;
use henad_core::authoring::model::network_model::NetworkModel;
use henad_core::params::ParamValue;

/// Half-width of the acceptance band, in standard deviations of a binomial proportion.
const SIGMAS: f64 = 5.0;

/// Returns the directory of the reference fixtures for `model`.
///
/// `HENAD_FIXTURE_DIR` points the gate at a directory holding one engine's candidates, so a
/// failure identifies that engine and no tracked fixture is written or removed to find out.
pub fn fixture_dir(model: &str) -> PathBuf {
    match std::env::var_os("HENAD_FIXTURE_DIR") {
        Some(root) => PathBuf::from(root).join(model),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(model),
    }
}

/// Returns the name and text of every `.txt` file of the fixture directory `dir`, sorted by name.
///
/// Panics if the directory or a file does not read.
pub fn reference_fixtures(dir: &Path) -> Vec<(String, String)> {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
    let mut fixtures = Vec::new();
    for entry in entries {
        let path = entry.expect("readable directory entry").path();
        if path.extension().is_none_or(|e| e != "txt") {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {name}: {e}"));
        fixtures.push((name, text));
    }
    fixtures.sort();
    fixtures
}

/// Returns the default params of the network model `M` with some overridden by id.
///
/// # Panics
///
/// Panics if an id is not declared by the model.
pub fn network_params<M: NetworkModel>(overrides: &[(&str, ParamValue)]) -> Vec<ParamValue> {
    let descs = network_model_param_descriptors::<M>();
    for (id, _) in overrides {
        assert!(descs.iter().any(|d| d.id() == *id), "no parameter '{id}'");
    }
    descs
        .iter()
        .map(|d| {
            overrides
                .iter()
                .find(|(id, _)| *id == d.id())
                .map_or_else(|| d.kind().default_value(), |(_, v)| v.clone())
        })
        .collect()
}

/// Returns the position of the parameter `id` among the params of the network model `M`.
///
/// # Panics
///
/// Panics if the model declares no parameter `id`.
pub fn param_index<M: NetworkModel>(id: &str) -> usize {
    network_model_param_descriptors::<M>()
        .iter()
        .position(|d| d.id() == id)
        .unwrap_or_else(|| panic!("no parameter '{id}'"))
}

/// Returns the half-width of the acceptance band for a binomial proportion `expected` over `trials` trials.
pub fn band(expected: f64, trials: u64) -> f64 {
    SIGMAS * (expected * (1.0 - expected) / trials as f64).sqrt()
}
