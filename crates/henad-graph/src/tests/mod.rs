//! Tests of the reader, the canonical writer, the code identity, the catalogue, the library lock and the checker, and
//! of the sheet reader, writer, checker and lowering.

mod agent_rules;
mod agent_wgsl;
mod case_files;
mod catalogue;
mod checker;
mod corpus;
mod edit;
mod file_format;
mod functions;
mod gpu_cases;
mod gpu_repeat;
mod host;
mod known_answers;
mod library_lock;
mod live_params;
mod mutation;
mod packing;
mod probes;
mod regions;
mod sheet_agents;
mod sheet_check;
mod sheet_convert;
pub(crate) mod sheet_document;
mod sheet_edit;
mod sheet_fires;
mod sheet_ground;
mod sheet_lower;
mod sheet_mutation;
mod sheet_rates;
mod sheet_repeat;
mod sheet_sentence;
mod sheet_writer;
mod sum_fields;
mod support;
mod time_scale;
mod wgsl;
