//! Graph models: the node graph a model file of kind `graph` holds, read and written in one canonical form.
//!
//! A graph file holds header keys, a `[graph]` table, one `[[target]]` table, a world table, declaration tables, a
//! stack of `[[pass]]` tables holding `[[pass.node]]` tables, embedded `[[function]]` tables and a trailing `[editor]`
//! region. [`GraphFile::parse`] reads one into a [`document::GraphDocument`], and records the place of every table and
//! key in [`file::Places`].
//!
//! The *canonical text* is the file written again by [`GraphFile::to_toml`]: every table in a fixed order, every key in
//! a fixed order and every value in one form. Two spellings of one graph have the same canonical text. The canonical
//! text before `[editor]` is the graph's code identity, and [`GraphFile::code`] returns it as a
//! [`ModelCode`](henad_core::provenance::ModelCode) in the format [`CODE_FORMAT`]. Moving a node therefore leaves the
//! identity alone.
//!
//! ```
//! use henad_graph::GraphFile;
//!
//! let text = r#"
//! format = "henad-model"
//! format_version = 1
//! kind = "graph"
//! id = "node_empty"
//!
//! [graph]
//! topology = "grid"
//! library = 1
//!
//! [[target]]
//! backend = "gpu"
//! "#;
//! let file = GraphFile::parse(text).expect("the file reads");
//! assert_eq!(file.document().target.id, "node_empty");
//! assert_eq!(file.code().format(), henad_graph::CODE_FORMAT);
//! ```

#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]
// Proving a type that holds wgpu handles `Send` or `Sync` walks wgpu-core's registries, deeper than the default
// limit of 128.
#![recursion_limit = "256"]

pub mod catalogue;
pub mod check;
pub mod document;
pub mod draw;
pub mod edit;
pub mod editor;
mod expr;
pub mod file;
pub mod host;
pub mod ids;
pub mod limits;
pub mod model_text;
pub mod node;
pub mod normal;
mod reader;
pub mod starters;
pub mod types;
pub mod value;
pub mod wgsl;
mod writer;

#[cfg(test)]
mod tests;

pub use file::{CODE_FORMAT, FORMAT, FORMAT_VERSION, GraphFile, GraphFileError, KIND_GRAPH, LIBRARY};
