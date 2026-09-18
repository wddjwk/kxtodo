//! 固定导航里「钉住的工具」的 id 清单一致性（v0.8.3）。
//!
//! core 的 `model::NAV_TOOL_IDS` 与前端 `src/lib/tools/catalog.ts` 的 `TOOL_CATALOG`
//! 是同一份清单的两份拷贝：core 要在 `config set appearance.navItems` 时挡住拼错的
//! `tool:<id>`，而它不 import 前端。只改一边的后果是「界面钉得进去、core 拒收」
//! （或反过来，core 收了一个界面画不出来的行）。
//! 与 `ledger_icons.rs` 的镜像测试同一套路：把前端那份 TS 当字符串读进来逐个比对。

use kxtodo_core::model::NAV_TOOL_IDS;

const CATALOG: &str = include_str!("../../../../src/lib/tools/catalog.ts");

fn catalog_ids() -> Vec<String> {
    CATALOG
        .lines()
        .filter_map(|line| line.trim().strip_prefix("id: \""))
        .filter_map(|rest| rest.split('"').next().map(str::to_string))
        .collect()
}

#[test]
fn core_and_frontend_agree_on_pinnable_tools() {
    let frontend = catalog_ids();
    assert!(!frontend.is_empty(), "没从 catalog.ts 里解析出任何工具 id");
    for id in NAV_TOOL_IDS {
        assert!(
            frontend.iter().any(|entry| entry == id),
            "core 的 NAV_TOOL_IDS 里有 `{id}`，但前端 catalog.ts 里没有这一项"
        );
    }
    for id in &frontend {
        assert!(
            NAV_TOOL_IDS.contains(&id.as_str()),
            "前端 catalog.ts 里有 `{id}`，但 core 的 NAV_TOOL_IDS 没跟上"
        );
    }
}
