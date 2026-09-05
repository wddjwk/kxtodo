//! 后台静默运行（v0.5.0）：`--daemon` 以分离/无窗口方式重新拉起自己，`--stop` 结束它。
//!
//! - Linux：`process_group(0)` + `Stdio::null()`——新进程组不会收到终端的 SIGHUP，
//!   关掉终端也继续跑（与 core 里隐藏 Host 的做法一致）。
//! - Windows：`CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP`——没有控制台窗口，
//!   父进程退出后子进程照常运行。日志本来就双写到 `server/log/`，不依赖 stdout。
//!
//! pid 落在 `<数据目录>/server.pid`；`--stop` 会先确认那个 pid 真的是
//! kxtodo-server 才动手，避免拿着过期 pidfile 误杀别的进程。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn pid_file(data_dir: &Path) -> PathBuf {
    data_dir.join("server.pid")
}

pub fn write_pid(data_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(data_dir).map_err(|e| format!("创建数据目录失败：{e}"))?;
    std::fs::write(pid_file(data_dir), std::process::id().to_string())
        .map_err(|e| format!("写 pidfile 失败：{e}"))
}

pub fn remove_pid(data_dir: &Path) {
    let _ = std::fs::remove_file(pid_file(data_dir));
}

/// 以分离/无窗口方式重新拉起自己（参数原样转发，只摘掉 `--daemon`）。
pub fn spawn_detached() -> Result<u32, String> {
    let exe = std::env::current_exe().map_err(|e| format!("无法定位当前二进制：{e}"))?;
    let forwarded: Vec<std::ffi::OsString> = std::env::args_os()
        .skip(1)
        .filter(|arg| arg != "--daemon")
        .collect();
    let mut command = Command::new(exe);
    command
        .args(&forwarded)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 新进程组：终端关闭时的 SIGHUP 只发给前台进程组，不会带走它
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }

    let child = command
        .spawn()
        .map_err(|e| format!("拉起后台进程失败：{e}"))?;
    Ok(child.id())
}

/// 结束后台进程：确认 pid 属于 kxtodo-server 后再发信号。
pub fn stop(data_dir: &Path) -> Result<(), String> {
    let path = pid_file(data_dir);
    let raw = std::fs::read_to_string(&path)
        .map_err(|_| format!("没有找到 {}（服务器可能没在后台运行）", path.display()))?;
    let pid: u32 = raw
        .trim()
        .parse()
        .map_err(|_| format!("pidfile 内容无效：{}", raw.trim()))?;
    if !process_is_ours(pid) {
        remove_pid(data_dir);
        return Err(format!(
            "pid {pid} 不是 kxtodo-server（pidfile 已过期并清除），拒绝结束该进程"
        ));
    }
    terminate(pid)?;
    for _ in 0..50 {
        if !process_is_ours(pid) {
            remove_pid(data_dir);
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    remove_pid(data_dir);
    Err(format!("pid {pid} 未在 5 秒内退出，请手动检查"))
}

fn terminate(pid: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        let output = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F", "/T"])
            .output()
            .map_err(|e| format!("调用 taskkill 失败：{e}"))?;
        if !output.status.success() {
            return Err(format!(
                "taskkill 失败：{}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    #[cfg(not(windows))]
    {
        // SIGTERM：main.rs 里装了处理器，会走优雅停机并自己清 pidfile
        let output = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output()
            .map_err(|e| format!("调用 kill 失败：{e}"))?;
        if !output.status.success() {
            return Err(format!(
                "kill 失败：{}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    Ok(())
}

/// pid 是否真的属于 kxtodo-server（防止过期 pidfile 误杀无关进程）。
fn process_is_ours(pid: u32) -> bool {
    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
            .output();
        return match output {
            Ok(output) => String::from_utf8_lossy(&output.stdout).contains("kxtodo-server"),
            Err(_) => false,
        };
    }
    #[cfg(target_os = "linux")]
    {
        // comm 截断到 15 字符，"kxtodo-server" 正好 13 字符
        return std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .map(|name| name.trim() == "kxtodo-server")
            .unwrap_or(false);
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output();
        return match output {
            Ok(output) => {
                let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
                name.ends_with("kxtodo-server")
            }
            Err(_) => false,
        };
    }
}
