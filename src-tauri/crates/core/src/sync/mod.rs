//! 数据多端同步（v0.4.0）：端到端加密 + 逐实体 LWW + 墓碑。
//! v0.5.0：局域网自动发现（discovery）+ 图片文件同步（images）。
//! v0.5.1：账户标识简化为「用户名 + 密码」（不再拼邮箱）+ 配对历史（history）。
//! v0.6.0：传输分层——三种通信方式（局域网内置主机 / 自建服务 / P2P）共用同一套
//! HTTP 协议（transport）与同一套合并逻辑（merge），差别只在 base url 从哪来（endpoint）。
//! v0.6.1：P2P 落地（p2p）——iroh 承载 + 账户派生的 pkarr 设备目录 + 枢纽规则。

pub mod crypto;
pub mod discovery;
pub mod endpoint;
pub mod engine;
pub mod history;
pub mod images;
pub mod merge;
pub mod p2p;
pub mod state;
pub mod transport;
