//! Bastion's server (DECISIONS.md D27): the factory and everything it runs on,
//! reached through the Engine seam, with the HTTP API in front of it.

pub mod access;
pub mod api;
pub mod audit_log;
pub mod charter;
pub mod console;
pub mod doctor;
pub mod engine;
pub mod eval_store;
pub mod evals;
pub mod events;
pub mod factory;
pub mod forge;
pub mod gate;
pub mod govern_store;
pub mod http;
pub mod lessons;
pub mod machine;
pub mod notify;
pub mod people_store;
pub mod rt;
pub mod seed_flows;
pub mod server;
pub mod settings_store;
pub mod stack;
pub mod store;
pub mod tls;
pub mod workflow_store;
