//! Building blocks for parameter sweeps over a model.
//!
//! A config is one set of parameter values and action ticks. A run builds the model at a config with one seed and
//! steps it, and the runs of a config are its replicates, told apart by their seeds. A sample reads the model's
//! stats at one tick, and a reducer folds a run's samples of one stat column into one value. A stop condition ends a
//! run at the first sample where it holds. A factor is a parameter or action tick a sweep varies, and a block
//! combines factors under one design. The configs of a sweep are those of its blocks, in order. A search picks its
//! configs a batch at a time instead, from the results of the configs it ran before.

pub mod design;
pub mod design_csv;
pub mod design_rng;
pub mod factor;
pub mod fingerprint;
pub mod measure;
pub mod outcome;
pub mod plan;
pub mod reducer;
pub mod replay;
pub mod search;
pub mod seed;
pub mod spec;
pub mod stop;
pub mod summary;
pub mod value;
