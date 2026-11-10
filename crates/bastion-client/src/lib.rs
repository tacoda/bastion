//! A client of Bastion servers (PLAN.md D38): what every interface needs to
//! reach a server through its API and its events, and nothing about how
//! it shows them.

pub mod client;
pub mod events;
pub mod local;
pub mod servers;
pub mod start;
