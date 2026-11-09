//! What each client may do, one module per resource (D28). The HTTP routes
//! call these.

pub mod console;
pub mod doctor;
pub mod engine;
pub mod evals;
pub mod factory;
pub mod govern;
pub mod hosts;
pub mod people;
pub mod providers;
pub mod settings;
pub mod sign_in;
pub mod stack;
pub mod workflows;
