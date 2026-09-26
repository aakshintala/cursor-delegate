// Test seams are injected as `dyn Fn` fields, and the poll/dispatch results are short-lived
// values returned once per call.
#![allow(clippy::type_complexity, clippy::large_enum_variant)]

pub mod backends;
pub mod capability;
pub mod config;
pub mod cursor_bin;
pub mod doctor;
pub mod finalize;
pub mod gate;
pub mod git;
pub mod index;
pub mod isolation;
pub mod job_registry;
pub mod models;
pub mod output;
pub mod pricing;
pub mod progress;
pub mod prompt;
pub mod runner;
pub mod safety;
pub mod status_record;
pub mod stream;
pub mod tool_schemas;
pub mod types;
pub mod util;
pub mod validate;
