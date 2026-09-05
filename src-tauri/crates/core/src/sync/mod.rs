//! 数据多端同步（v0.4.0）：端到端加密 + 逐实体 LWW + 墓碑。
//! v0.5.0：局域网自动发现（discovery）+ 图片文件同步（images）。
//! v0.5.1：账户标识简化为「用户名 + 密码」（不再拼邮箱）+ 配对历史（history）。

pub mod crypto;
pub mod discovery;
pub mod engine;
pub mod history;
pub mod images;
pub mod merge;
pub mod state;
