//! KXToDo v9: CLI, Domain Core, Repository, Background Host.
//! Desktop-only module; all GUI-shared behavior lives here (requirements §4.1).

pub mod cli;
pub mod core;
pub mod doctor;
pub mod envelope;
pub mod error;
pub mod exec;
pub mod history;
pub mod host;
pub mod ids;
pub mod ipc;
pub mod jq;
pub mod migrate;
pub mod model;
pub mod ops_config;
pub mod ops_gui;
pub mod ops_schedule;
pub mod ops_task;
pub mod plan;
pub mod render;
pub mod repo;
pub mod scheduler;
pub mod schema;
pub mod skills;
pub mod time;

pub use error::CoreError;
