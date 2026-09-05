use std::process::Command;

/// 版本号的唯一来源是 git，优先级：HEAD 上的精确 v* tag → 最近一条以 vX.Y.Z 开头的
/// commit subject → 最近的祖先 v* tag。构建期注入 KXTODO_VERSION，仓库文件里不再维护版本号。
fn git_version() -> Option<String> {
    // 精确 tag 必须排在祖先 tag 之前：describe --abbrev=0 在未打 tag 的 commit 上返回的是
    // 上一个 tag，那样 subject 里写的版本号永远轮不到，构建会静默报旧版本号。
    let exact = run_git(&["describe", "--tags", "--match", "v*", "--exact-match"]);
    if let Some(version) = exact.as_deref().and_then(parse_version) {
        return Some(version);
    }
    if let Some(log) = run_git(&["log", "-30", "--pretty=%s"]) {
        if let Some(version) = log.lines().find_map(parse_version) {
            return Some(version);
        }
    }
    run_git(&["describe", "--tags", "--match", "v*", "--abbrev=0"])
        .as_deref()
        .and_then(parse_version)
}

fn run_git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
}

/// "v0.4.2" / "v0.4.2 修复同步水位" → "0.4.2"；不是小写 v + 三段纯数字则 None。
/// 只取第一个空白前的 token——拿整行去校验会被后面的说明文字带崩（旧实现的 bug：
/// 整行 split('.') 的最后一段必然含中文，导致这条回退对所有真实 subject 都失效）。
fn parse_version(text: &str) -> Option<String> {
    let candidate = text.trim().split_whitespace().next()?.strip_prefix('v')?;
    let parts: Vec<&str> = candidate.split('.').collect();
    let valid = parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
    valid.then(|| candidate.to_string())
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
