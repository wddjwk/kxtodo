//! 更新下载通道的纯逻辑回归：代理拼接格式与候选顺序（真机测速不在这里测，避免网络依赖）。

use kxtodo_core::update_fetch::{download_routes, speed_label, GITHUB_PROXY_PREFIXES};

const ASSET: &str = "https://github.com/wddjwk/kxtodo/releases/download/v0.7.5/KXToDo.apk";

#[test]
fn routes_prefix_full_github_url() {
    let routes = download_routes(ASSET);
    assert_eq!(routes.len(), GITHUB_PROXY_PREFIXES.len() + 1);
    for (route, prefix) in routes.iter().zip(GITHUB_PROXY_PREFIXES.iter()) {
        // 用法是把完整的 github.com 链接拼在代理域名后面（不是 api 域名）
        assert_eq!(route.url, format!("{prefix}{ASSET}"));
        assert!(!route.official);
        assert!(route.url.contains("github.com/wddjwk/kxtodo/releases/download/"));
        assert!(!route.url.contains("api.github.com"));
    }
}

#[test]
fn official_route_is_last_in_candidate_list() {
    let routes = download_routes(ASSET);
    let official = routes.last().expect("至少有一条候选");
    assert!(official.official);
    assert_eq!(official.url, ASSET);
    assert_eq!(official.label, "GitHub 直连");
}

#[test]
fn ghfast_kept_and_speed_label_readable() {
    let routes = download_routes(ASSET);
    assert!(routes.iter().any(|route| route.url.starts_with("https://ghfast.top/")));
    assert_eq!(speed_label(None), "测速失败");
    assert_eq!(speed_label(Some(2.0 * 1024.0 * 1024.0)), "2.0 MB/s");
    assert_eq!(speed_label(Some(512.0 * 1024.0)), "512 KB/s");
}
