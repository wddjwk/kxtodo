//! `--update` 自升级：检查 GitHub latest release → 下载本平台固定名 kxtodo-server
//! （Windows 为 kxtodo-server.exe）→ 原子替换自身 → 以 `--update-restarted` 拉起新版本
//! （子进程重试绑定端口）→ 退出。
//!
//! 替换策略：下载到 `<exe>.new` → 当前二进制改名 `<exe>.old` → `.new` 落位。
//! 运行中的二进制允许被改名（Windows 也允许 rename，只是不能删除）。

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

pub const APP_VERSION: &str = env!("KXTODO_VERSION");
const GITHUB_LATEST_API: &str = "https://api.github.com/repos/wddjwk/kxtodo/releases/latest";
#[cfg(windows)]
const GITHUB_LATEST_ASSET: &str =
    "https://github.com/wddjwk/kxtodo/releases/latest/download/kxtodo-server.exe";
#[cfg(not(windows))]
const GITHUB_LATEST_ASSET: &str =
    "https://github.com/wddjwk/kxtodo/releases/latest/download/kxtodo-server";
/// 最小合法产物体积（防半截下载 / HTML 错误页被当成二进制）
const MIN_BINARY_BYTES: u64 = 1_000_000;

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        // 刻意不设总超时（产物几十 MB），但读必须有：对端连上之后不再发数据时不会永远挂着
        .timeout_read(Duration::from_secs(60))
        .build()
}

fn latest_version() -> Result<String, String> {
    let response = agent()
        .get(GITHUB_LATEST_API)
        .call()
        .map_err(|error| format!("无法访问 GitHub API：{error}"))?;
    let body: Value = response
        .into_json()
        .map_err(|error| format!("latest release 响应无效：{error}"))?;
    body.get("tag_name")
        .and_then(Value::as_str)
        .map(|tag| tag.trim().trim_start_matches('v').to_string())
        .filter(|tag| !tag.is_empty())
        .ok_or_else(|| "latest release 缺少 tag_name".to_string())
}

/// GitHub 直连失败时回退的下载代理：把完整 GitHub 链接拼在它后面即可。
/// （实测该代理只接受 github.com 的下载链接，api.github.com 会 403，
///  所以版本检查仍然直连 API，只有**下载**走回退。）
const GITHUB_PROXY: &str = "https://ghfast.top/";

/// 下载到 `<path>.part` 再原子改名落位：半截产物绝不会顶上正在跑的二进制。
/// 直连失败（连不上/截断/体积不对）会清掉残留后换代理重试，两条路都失败则一起报出来。
fn download_to(path: &std::path::Path) -> Result<(), String> {
    let part = path.with_extension("part");
    if let Err(direct_error) = stream_to(GITHUB_LATEST_ASSET, &part) {
        let _ = std::fs::remove_file(&part);
        let proxied = format!("{GITHUB_PROXY}{GITHUB_LATEST_ASSET}");
        println!("GitHub 直连失败（{direct_error}），改用代理重试：{proxied}");
        stream_to(&proxied, &part).map_err(|proxy_error| {
            let _ = std::fs::remove_file(&part);
            format!(
                "下载 kxtodo-server 失败\n  直连：{direct_error}\n  代理（{GITHUB_PROXY}）：{proxy_error}"
            )
        })?;
    }
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    std::fs::rename(&part, path).map_err(|error| {
        let _ = std::fs::remove_file(&part);
        format!("新版本落位失败：{error}")
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// 流式下载并打进度（每 10% 一行）。体积不对或下载被截断都算失败，`.part` 由调用方清理。
fn stream_to(url: &str, part: &std::path::Path) -> Result<(), String> {
    use std::io::{Read, Write};
    let response = agent()
        .get(url)
        .call()
        .map_err(|error| format!("请求失败：{error}"))?;
    let total: u64 = response
        .header("Content-Length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    if total > 0 && total < MIN_BINARY_BYTES {
        return Err(format!(
            "下载产物过小（{total} 字节），疑似无效资产；本机平台制品可能尚未发布 kxtodo-server(.exe)"
        ));
    }
    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(part).map_err(|error| format!("创建临时文件失败：{error}"))?;
    let mut buffer = [0u8; 64 * 1024];
    let mut received: u64 = 0;
    let mut last_step: u64 = 0;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| format!("下载中断：{error}"))?;
        if count == 0 {
            break;
        }
        file.write_all(&buffer[..count])
            .map_err(|error| format!("写入临时文件失败：{error}"))?;
        received += count as u64;
        if total > 0 {
            let step = received * 10 / total;
            if step != last_step {
                last_step = step;
                println!("下载中 {}%（{received}/{total} 字节）", step * 10);
            }
        }
    }
    file.flush().map_err(|error| format!("写入临时文件失败：{error}"))?;
    if received < MIN_BINARY_BYTES {
        return Err(format!(
            "下载内容过小（{received} 字节），疑似无效资产"
        ));
    }
    if total > 0 && received != total {
        return Err(format!("下载不完整（{received}/{total} 字节）"));
    }
    Ok(())
}

fn version_gt(candidate: &str, current: &str) -> bool {
    let parse = |raw: &str| -> Vec<u64> {
        raw.split('.')
            .map(|part| part.trim().parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parse(candidate), parse(current));
    for index in 0..a.len().max(b.len()) {
        let x = a.get(index).copied().unwrap_or(0);
        let y = b.get(index).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

fn current_exe_path() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|error| format!("无法定位当前二进制：{error}"))
}

/// 执行升级。返回 Err 时进程不退出（打印错误继续运行）。
pub fn run_update(force: bool) -> Result<(), String> {
    let exe = current_exe_path()?;
    let latest = latest_version()?;
    if !force && !version_gt(&latest, APP_VERSION) {
        println!("当前已是最新版本 v{APP_VERSION}（远端 v{latest}）");
        return Ok(());
    }
    println!("发现新版本 v{latest}（当前 v{APP_VERSION}），开始下载…");

    let new_path = exe.with_extension("new");
    let old_path = exe.with_extension("old");
    download_to(&new_path)?;

    std::fs::remove_file(&old_path).ok();
    std::fs::rename(&exe, &old_path)
        .map_err(|error| format!("当前二进制无法改名（{error}）；请手动放置新版本"))?;
    std::fs::rename(&new_path, &exe)
        .map_err(|error| format!("新版本落位失败：{error}（旧版本保留在 .old）"))?;
    println!("已替换为 v{latest}，正在重启…");

    // 子进程带 --update-restarted 重试绑定端口；父进程退出释放端口。
    let mut command = std::process::Command::new(&exe);
    command.arg("--update-restarted");
    if let Ok(running_db) = std::env::var("KXTODO_SERVER_DB") {
        command.env("KXTODO_SERVER_DB", running_db);
    }
    if let Ok(running_listen) = std::env::var("KXTODO_SERVER_LISTEN") {
        command.arg("--listen").arg(&running_listen);
    }
    command
        .spawn()
        .map_err(|error| format!("拉起新版本失败：{error}；请手动重启 kxtodo-server"))?;
    std::process::exit(0);
}
