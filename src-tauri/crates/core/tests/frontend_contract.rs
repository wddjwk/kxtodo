//! 跨语言「孪生常量」的钉子（v0.8.3 review #11）。
//!
//! 这几组值在 Rust 与 TS 两侧各写了一份，靠注释说「两边要一致」——而注释不会失败。
//! 漂了的真实症状：
//! - 标签色名单少一个 → 那一色在界面上退成灰，或 core 收下的颜色前端画不出来；
//! - 背景图长边不一样 → 前端压过一道、壳再压一道，画质白丢一次；
//! - 临期配色档位数不一样 → 用户挑的第四色每次快照刷新被截掉（`normalizeDueColors`
//!   与 core 的 `expect_due_colors` 各写一个数字，v0.8.3 加「已过期」档时前端就漏过）。
//!
//! 手法照 `ledger_icons.rs`：把前端 TS 当字符串读进来比对。

mod common;

use common::TestEnv;
use kxtodo_core::model::TagColor;

const TYPES_TS: &str = include_str!("../../../../src/lib/types.ts");
const DEFAULTS_TS: &str = include_str!("../../../../src/lib/defaults.ts");
const TAG_COLORS_TS: &str = include_str!("../../../../src/lib/tagColors.ts");
const IMAGES_TS: &str = include_str!("../../../../src/lib/images.ts");
const DUE_TS: &str = include_str!("../../../../src/lib/dueHighlight.ts");
const SHELL_RS: &str = include_str!("../../../src/lib.rs");

/// 取 `start` 与随后第一个 `end` 之间的那一段（找不到就回空串，让断言去报「找不到了」）。
fn between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    let Some(from) = source.find(start) else {
        return "";
    };
    let from = from + start.len();
    let to = source[from..].find(end).map(|offset| from + offset).unwrap_or(source.len());
    &source[from..to]
}

/// 一段文本里所有双引号字符串，按出现顺序。
fn quoted(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = block;
    while let Some(open) = rest.find('"') {
        rest = &rest[open + 1..];
        let Some(close) = rest.find('"') else { break };
        out.push(rest[..close].to_string());
        rest = &rest[close + 1..];
    }
    out
}

/// core 的十种标签色（`TagColor::as_str` 的全集）——这一份是真相来源。
const RUST_TAG_COLORS: [&str; 10] = [
    "red", "orange", "yellow", "green", "cyan", "blue", "purple", "pink", "gray", "custom",
];

#[test]
fn tag_color_names_match_across_languages() {
    // 先确认这份名单确实等于 Rust 的枚举（parse 与 as_str 互逆，没有别名与漏项）
    for name in RUST_TAG_COLORS {
        assert_eq!(
            TagColor::parse(name).map(|color| color.as_str()),
            Some(name),
            "`{name}` 在 core 这边不是自洽的"
        );
    }
    assert_eq!(TagColor::parse("magenta"), None);

    let union = quoted(between(TYPES_TS, "export type TagColor =", ";"));
    assert_eq!(
        union, RUST_TAG_COLORS,
        "types.ts 的 TagColor 联合类型与 core 的 TagColor 不是同一份名单"
    );

    let whitelist = quoted(between(DEFAULTS_TS, "const TAG_COLORS: TagColor[] = [", "]"));
    assert_eq!(
        whitelist, RUST_TAG_COLORS,
        "defaults.ts 的 TAG_COLORS（normalize 的白名单，不在名单里一律退灰）与 core 不一致"
    );

    // 配色表刻意少 `custom`（自定义色走 hex，没有固定配色），其余九个必须齐且同序
    let specs: Vec<String> = between(TAG_COLORS_TS, "export const TAG_COLOR_SPECS", "];")
        .split("color: \"")
        .skip(1)
        .map(|chunk| chunk.split('"').next().unwrap_or_default().to_string())
        .collect();
    assert_eq!(
        specs,
        RUST_TAG_COLORS[..9],
        "tagColors.ts 的 TAG_COLOR_SPECS 少了/多了颜色（custom 除外，它没有固定配色）"
    );
}

#[test]
fn background_max_edge_matches_the_shell_gate() {
    let edge = between(IMAGES_TS, "export const BACKGROUND_MAX_EDGE =", ";").trim();
    assert!(
        !edge.is_empty() && edge.chars().all(|ch| ch.is_ascii_digit()),
        "前端 images.ts 的 BACKGROUND_MAX_EDGE 找不到了（改名了就把这里一起改）"
    );
    let gate = between(SHELL_RS, "ImageGate::Background =>", "\n");
    assert!(
        gate.contains(edge),
        "壳的背景图闸（src-tauri/src/lib.rs）没跟着改：{gate} 里找不到 {edge}"
    );
}

#[test]
fn due_color_palette_size_is_what_core_accepts() {
    let palette = quoted(between(DUE_TS, "export const DEFAULT_DUE_COLORS =", ";"));
    assert!(
        palette.len() >= 4,
        "前端默认配色应是四档（已过期/今天/明天/后天），实际 {} 档",
        palette.len()
    );
    let env = TestEnv::fresh();
    let list = |colors: &[String]| {
        format!(
            "{{\"entry-a\":[{}]}}",
            colors
                .iter()
                .map(|color| format!("\"{color}\""))
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    // 前端那份默认配色 core 必须原样收下（档位数对得上）
    let stored = env.ok(&["config", "set", "appearance.dueColors", &list(&palette)]);
    assert_eq!(stored["value"]["entry-a"].as_array().unwrap().len(), palette.len());
    // 少一档就拒：core 的档位数是写死的，前端加/减一档必须同步改 expect_due_colors
    env.err(
        &[
            "config",
            "set",
            "appearance.dueColors",
            &list(&palette[..palette.len() - 1]),
        ],
        2,
    );
}
