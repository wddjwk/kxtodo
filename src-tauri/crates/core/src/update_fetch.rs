//! 应用更新的下载通道选择：GitHub 官方直连 + 一组加速代理。
//!
//! 代理的用法只有一个：把**完整的 github.com 制品链接**拼在代理域名后面
//! （`https://gh-proxy.org/https://github.com/…/KXToDo.apk`）——走的是 github.com，
//! 不是 api.github.com（这些代理不接受 api 域名，版本检查仍必须直连 API）。
//!
//! 下载前把每条通道都量一遍速度，挑最快的那条去下载；失败按速度顺序换下一条，
//! 官方直连永远留在候选表里垫底兜底（测速失败也要留着试一把）。

use std::io::Read;
use std::time::{Duration, Instant};

/// 加速代理前缀。顺序只是探测顺序，实际按测得的速度排。
pub const GITHUB_PROXY_PREFIXES: [&str; 6] = [
    "https://gh-proxy.org/",
    "https://v4.gh-proxy.org/",
    "https://v6.gh-proxy.org/",
    "https://cdn.gh-proxy.org/",
    "https://axisnow.gh-proxy.org/",
    "https://ghfast.top/",
];

/// 测速样本：每条通道最多读这么多字节就掐掉（七条通道合计不到 2MB）
const PROBE_BYTES: usize = 256 * 1024;
/// 单条通道的测速上限：到点就放弃这一条（墙内直连 github.com 会一直挂着）
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// 有效样本下限：低于它当「没测到」（错误页只有几 KB，不能当成能下载）
const PROBE_MIN_BYTES: u64 = 64 * 1024;

/// 一条下载通道。
#[derive(Debug, Clone)]
pub struct DownloadRoute {
    /// 展示名（进度提示里给用户看的那一句）
    pub label: String,
    pub url: String,
    /// 官方直连：探测失败也要留在候选表里兜底
    pub official: bool,
}

/// 候选通道 = 六个加速代理 + 官方直连（官方在最后一位，兜底位置）。
pub fn download_routes(github_url: &str) -> Vec<DownloadRoute> {
    let mut routes: Vec<DownloadRoute> = GITHUB_PROXY_PREFIXES
        .iter()
        .map(|prefix| DownloadRoute {
            label: prefix.trim_end_matches('/').replace("https://", ""),
            url: format!("{prefix}{github_url}"),
            official: false,
        })
        .collect();
    routes.push(DownloadRoute {
        label: "GitHub 直连".to_string(),
        url: github_url.to_string(),
        official: true,
    });
    routes
}

/// 量一条通道的下载速度（字节/秒）：连不上、状态码不对、样本太小都算没测到。
/// 用 Range 只取开头一小段，读完就丢连接——不整份下载也能比出快慢。
pub fn probe_speed(agent: &ureq::Agent, url: &str) -> Option<f64> {
    let started = Instant::now();
    let response = agent
        .get(url)
        .set("Range", &format!("bytes=0-{}", PROBE_BYTES - 1))
        .timeout(PROBE_TIMEOUT)
        .call()
        .ok()?;
    let status = response.status();
    if status >= 300 {
        return None;
    }
    let mut reader = response.into_reader();
    let mut buffer = [0u8; 32 * 1024];
    let mut received: u64 = 0;
    while received < PROBE_BYTES as u64 {
        let want = ((PROBE_BYTES as u64 - received).min(buffer.len() as u64)) as usize;
        let count = reader.read(&mut buffer[..want]).ok()?;
        if count == 0 {
            break;
        }
        received += count as u64;
    }
    let elapsed = started.elapsed().as_secs_f64();
    if received < PROBE_MIN_BYTES || elapsed <= 0.0 {
        return None;
    }
    Some(received as f64 / elapsed)
}

/// 并发探测所有通道（每条最多 5 秒）→ 按速度从快到慢排序，连速度一起给出来。
/// 没测到速度的排在有速度的后面；官方直连若也没测到就留在最后兜底。
pub fn rank_routes_with_speeds(
    agent: &ureq::Agent,
    routes: &[DownloadRoute],
) -> Vec<(Option<f64>, DownloadRoute)> {
    let speeds: Vec<Option<f64>> = std::thread::scope(|scope| {
        let handles: Vec<_> = routes
            .iter()
            .map(|route| {
                let agent = agent.clone();
                let url = route.url.clone();
                scope.spawn(move || probe_speed(&agent, &url))
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or(None))
            .collect()
    });
    let mut ranked: Vec<(Option<f64>, DownloadRoute)> =
        speeds.into_iter().zip(routes.iter().cloned()).collect();
    // 稳定排序：速度相同或都没测到时保持候选表原序（官方直连因此在最后）
    ranked.sort_by(|left, right| match (left.0, right.0) {
        (Some(a), Some(b)) => b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    ranked
}

/// 只要顺序的简版（GUI/APK 那条路：进度提示自己说，不需要逐条日志）。
pub fn rank_routes(agent: &ureq::Agent, routes: &[DownloadRoute]) -> Vec<DownloadRoute> {
    rank_routes_with_speeds(agent, routes)
        .into_iter()
        .map(|(_, route)| route)
        .collect()
}

/// 测速结果的展示串：给日志与进度提示用。
pub fn speed_label(speed: Option<f64>) -> String {
    match speed {
        Some(value) if value >= 1024.0 * 1024.0 => format!("{:.1} MB/s", value / (1024.0 * 1024.0)),
        Some(value) => format!("{:.0} KB/s", value / 1024.0),
        None => "测速失败".to_string(),
    }
}
