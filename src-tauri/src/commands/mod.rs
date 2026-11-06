//! One file per resource (PLAN.md D9).

pub mod app;
pub mod console;
pub mod doctor;
pub mod engine;
pub mod evals;
pub mod factory;
pub mod govern;
pub mod settings;
pub mod stack;

/// What the command wrappers name: their state, the page's types, and the
/// server's functions they call.
mod prelude {
    pub(crate) use std::sync::Arc;

    pub(crate) use bastion_core::Pipeline;
    pub(crate) use bastion_protocol::*;
    pub(crate) use bastion_server::api::{self, factory::FactoryCell};
    pub(crate) use bastion_server::engine::Engine;
    pub(crate) use bastion_server::events::Bus;
    pub(crate) use bastion_server::machine::Machine;
    pub(crate) use bastion_server::stack::Stack;
    pub(crate) use tauri::{AppHandle, Manager, State};

    pub(crate) use crate::profile::Profiles;
}
