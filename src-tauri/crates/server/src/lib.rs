//! kxtodo-server 的库入口（v0.6.0）。
//!
//! 独立二进制（`src/main.rs`）与 GUI/APK 的**内置主机**共用这里的一切。
//! 进程级动作（pidfile、`--daemon`、`--stop`、信号处理、退出码、`--update` 自升级）
//! 只在二进制入口里做——内置主机不拥有进程，碰这些会误伤宿主。
//!
//! 嵌入用法见 [`host::start`]。

pub mod admin;
pub mod api;
pub mod daemon;
pub mod db;
pub mod discovery;
pub mod error;
pub mod host;
pub mod logging;
pub mod metrics;
pub mod settings;
pub mod update;
pub mod util;

pub use error::{ServerError, ServerResult};
pub use host::{start, AdminProvision, ServerConfig, ServerHandle, StartError};
