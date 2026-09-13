//! 图标目录镜像的一致性（v0.7.3）。
//!
//! core 的 `ledger_icons.rs` 是前端 `src/lib/ledgerIcons.ts` 的镜像：CLI 的
//! `ledger icon-list` 把目录吐给 Agent 设计分类，界面按名字找 lucide 组件。
//! 只改一边的后果是「CLI 说能用、界面画不出来」（或反过来，界面有的图标 CLI 不认），
//! 所以这里直接把前端那份 TS 当字符串读进来逐个比对。
//!
//! 顺带守住一条更要命的：**种子账本用到的每个图标都必须在目录里**——
//! 种子分类/账户是首次启动就落盘的，图标不在目录里会一律退化成省略号。

use kxtodo_core::ledger_icons::{
    all_account_icons, all_icons, is_known_account_icon, is_known_icon, ACCOUNT_ICON_GROUPS,
    ICON_GROUPS,
};
use kxtodo_core::model::LedgerFile;

const TS: &str = include_str!("../../../../src/lib/ledgerIcons.ts");

#[test]
fn every_catalog_icon_exists_in_the_frontend() {
    let mut missing = Vec::new();
    for group in ICON_GROUPS {
        for icon in group.icons {
            // 前端是一份 `"Name": Name` 的记录 + 分组数组，两处都带引号
            if !TS.contains(&format!("\"{icon}\"")) {
                missing.push(format!("{}/{}", group.name, icon));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "这些图标前端没有（界面会画成省略号）：{}",
        missing.join(", ")
    );
}

#[test]
fn every_frontend_group_is_mirrored() {
    // 前端 `LEDGER_ICON_GROUPS` 里的 `{ name: "饮食", icons: [...] }` 一个都不能少
    for name in ["饮食", "娱乐", "购物", "交通", "金融理财", "收入", "通用"] {
        assert!(TS.contains(&format!("name: \"{name}\"")), "前端少了分组 {name}");
        assert!(
            ICON_GROUPS.iter().any(|group| group.name == name),
            "core 的目录少了分组 {name}"
        );
    }
    assert_eq!(ICON_GROUPS.len(), 20, "分组数与前端对不上");
}

#[test]
fn catalog_has_no_duplicates() {
    let icons = all_icons();
    let mut seen = std::collections::HashSet::new();
    for icon in &icons {
        assert!(seen.insert(*icon), "图标 {icon} 出现在多个分组里");
    }
    assert!(icons.len() >= 200, "图标目录被削了：只有 {} 个", icons.len());
}

#[test]
fn seeded_ledger_only_uses_known_icons() {
    let mut file = LedgerFile::default();
    file.seed_defaults();
    for category in &file.categories {
        if category.icon.is_empty() {
            continue; // 子分类留空 = 继承大类的颜色，图标由前端兜底
        }
        assert!(
            is_known_icon(&category.icon),
            "种子分类「{}」的图标 {} 不在目录里",
            category.name,
            category.icon
        );
    }
    for account in &file.accounts {
        assert!(
            is_known_icon(&account.icon),
            "种子账户「{}」的图标 {} 不在目录里",
            account.name,
            account.icon
        );
    }
}

#[test]
fn every_account_icon_exists_in_the_frontend() {
    let mut missing = Vec::new();
    for group in ACCOUNT_ICON_GROUPS {
        for icon in group.icons {
            if !TS.contains(&format!("\"{icon}\"")) {
                missing.push(format!("{}/{}", group.name, icon));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "这些账户图标前端没有（界面会画成省略号）：{}",
        missing.join(", ")
    );
}

#[test]
fn every_frontend_account_group_is_mirrored_in_order() {
    // 定位前端的 LEDGER_ACCOUNT_ICON_GROUPS 数组，逐个分组、逐个图标按顺序比对。
    // 顺序敏感：既能抓到「少了/多了图标」，也能抓到「组内或组间顺序漂移」。
    let start = TS
        .find("LEDGER_ACCOUNT_ICON_GROUPS")
        .expect("前端少了 LEDGER_ACCOUNT_ICON_GROUPS 导出");
    let body = &TS[start..];
    let end = body.find("];").expect("LEDGER_ACCOUNT_ICON_GROUPS 没有闭合");
    let block = &body[..end];
    let mut cursor = 0usize;
    for group in ACCOUNT_ICON_GROUPS {
        let name_token = format!("name: \"{}\"", group.name);
        let at = block[cursor..]
            .find(&name_token)
            .unwrap_or_else(|| panic!("前端账户图标目录缺少分组 {}（或顺序与 core 不一致）", group.name));
        cursor += at + name_token.len();
        for icon in group.icons {
            let icon_token = format!("\"{icon}\"");
            let at = block[cursor..].find(&icon_token).unwrap_or_else(|| {
                panic!(
                    "前端账户分组 {} 缺少图标 {}（或顺序与 core 不一致）",
                    group.name, icon
                )
            });
            cursor += at + icon_token.len();
        }
    }
    assert_eq!(ACCOUNT_ICON_GROUPS.len(), 6, "账户图标分组数与前端对不上");
}

#[test]
fn account_catalog_has_no_duplicates() {
    let icons = all_account_icons();
    let mut seen = std::collections::HashSet::new();
    for icon in &icons {
        assert!(seen.insert(*icon), "账户图标 {icon} 出现在多个分组里");
    }
    assert!(
        icons.len() >= 30,
        "账户图标目录被削了：只有 {} 个",
        icons.len()
    );
}

#[test]
fn seeded_ledger_accounts_only_use_known_account_icons() {
    let mut file = LedgerFile::default();
    file.seed_defaults();
    for account in &file.accounts {
        assert!(
            is_known_account_icon(&account.icon),
            "种子账户「{}」的图标 {} 不在账户目录里",
            account.name,
            account.icon
        );
    }
}
