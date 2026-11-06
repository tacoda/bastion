//! What each client may do, one module per resource (D28). The HTTP routes
//! and, until S3, the Tauri commands both call these.

pub mod console;
pub mod doctor;
pub mod engine;
pub mod evals;
pub mod factory;
pub mod govern;
pub mod people;
pub mod settings;
pub mod stack;
