//! 剪切板发现的无界面探针（v0.8.10）。
//!
//! 用途：两台机器（或 CI）跑同一份 core，验证局域网广播、统一服务器目录和 P2P
//! 房间的发现链路。它不读也不写系统剪切板，只打印发现到的设备。
//!
//! 例：
//!   cargo run -p kxtodo-core --example clipboard_probe -- --lan --name 书房 --duration 20
//!   cargo run -p kxtodo-core --example clipboard_probe -- --server http://192.168.1.10:52177 \
//!     --account demo --password demo-pass --duration 30
//!
//! 输出每行一个 JSON 状态快照（`peers` 字段是发现结果），便于脚本断言。

use std::path::PathBuf;
use std::time::Duration;

use kxtodo_core::clipboard_sync;
use kxtodo_core::model::SettingsFile;
use kxtodo_core::repo::Layout;

fn arg_value(args: &[String], name: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == name {
            return iter.next().cloned();
        }
        if let Some(value) = arg.strip_prefix(&format!("{name}=")) {
            return Some(value.to_string());
        }
    }
    None
}

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let data_dir = arg_value(&args, "--data-dir")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("kxtodo-probe-{}", std::process::id())));
    let duration = arg_value(&args, "--duration")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let layout = Layout::new(data_dir.clone());
    std::fs::create_dir_all(&data_dir).expect("data dir");
    let mut settings = SettingsFile::default();
    settings.clipboard.enabled = true;
    settings.clipboard.device_name = arg_value(&args, "--name").unwrap_or_default();
    settings.clipboard.lan_enabled = !has(&args, "--no-lan");
    settings.clipboard.server_url = arg_value(&args, "--server").unwrap_or_default();
    settings.clipboard.server_enabled = !settings.clipboard.server_url.is_empty();
    settings.clipboard.account = arg_value(&args, "--account").unwrap_or_default();
    settings.clipboard.password = arg_value(&args, "--password").unwrap_or_default();
    settings.clipboard.p2p_enabled = has(&args, "--p2p");
    clipboard_sync::reconcile(&layout, &settings, None).expect("start clipboard service");
    let started = std::time::Instant::now();
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let value = clipboard_sync::status(&layout, &settings);
        println!("{}", serde_json::to_string(&value).unwrap_or_default());
        if duration > 0 && started.elapsed() >= Duration::from_secs(duration) {
            break;
        }
    }
    clipboard_sync::stop_for_layout(&layout);
}
