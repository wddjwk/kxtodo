//! KXToDo Domain Core：数据模型、仓储、业务命令、CLI、Background Host。
//! GUI（kxtodo）与 CLI（kxtodo-cli）共享的纯 Rust 层，不依赖 Tauri。

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
