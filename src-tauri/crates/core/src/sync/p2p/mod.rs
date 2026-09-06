//! P2P 同步（v0.6.1）：iroh（QUIC + 打洞 + n0 公共 relay）承载，
//! 「**这一轮连哪儿**」由账户派生的 pkarr 目录回答。
//!
//! 角色规则（无需选举、无需协商）：
//! - 目录 = 同一账户的所有设备用一个**账户派生密钥**签的一条 pkarr TXT 记录，
//!   每台设备把自己的 EndpointId 读-改-写进去；任何设备只凭用户名密码就能解析出兄弟设备。
//! - 枢纽 = 目录在线设备（含自己）里 **EndpointId 最小**的那台。每轮只连一个端点：
//!   自己是枢纽就连自己的内置库（回环），否则拨号枢纽、在 iroh 隧道里对它做一次
//!   完整的普通 HTTP 同步（被叫方把隧道接到自己的内置服务器）。
//! - 枢纽下线时目录条目过期（15 分钟）+ 拨号失败冷却（5 分钟），下一台自动顶上；
//!   换枢纽 = 换主机库 = 既有的 epoch 换档逻辑（水位/台账清零重新播种）。
//!
//! 分层不变式：**改「连哪儿」不许动 merge/crypto**——这里只产出 base_url，
//! 隧道里跑的仍是 transport.rs 那 12 个 HTTP 方法。

pub mod directory;
pub mod identity;
pub mod net;

pub use net::{current, current_for, start, stop, stop_for, stop_for_layout, P2pConfig, P2pRuntime, Tunnel};
