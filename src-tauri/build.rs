use std::process::Command;

/// 版本号的唯一来源是 git：最近的 v* tag 优先，其次最近一条以 vX.Y.Z 开头的
/// commit message。构建期注入 KXTODO_VERSION，仓库文件里不再维护版本号。
fn git_version() -> Option<String> {
    let tag = Command::new("git")
        .args(["describe", "--tags", "--match", "v*", "--abbrev=0"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().trim_start_matches('v').to_string())
        .filter(|v| !v.is_empty());
    if tag.is_some() {
        return tag;
    }
    let log = Command::new("git")
        .args(["log", "-30", "--pretty=%s"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())?;
    for line in log.lines() {
        let Some(candidate) = line.trim().strip_prefix('v') else {
            continue;
        };
        let valid = !candidate.is_empty()
            && candidate
                .split('.')
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
        if valid {
            return Some(candidate.to_string());
        }
    }
    None
}

fn main() {
    let version = git_version().unwrap_or_else(|| "0.0.0-dev".to_string());
    println!("cargo:rustc-env=KXTODO_VERSION={version}");
    // tag/HEAD 变化时重编
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/packed-refs");
    println!("cargo:rerun-if-changed=../.git/refs/tags");
    tauri_build::build()
}
