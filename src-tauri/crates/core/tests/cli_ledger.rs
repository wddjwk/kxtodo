#![recursion_limit = "512"]
//! ledger domain end-to-end tests：记账的增删改查、转账与余额、统计、Excel 导出导入往返。

mod common;

use common::TestEnv;
use serde_json::Value;

fn add(env: &TestEnv, args: &[&str]) -> Value {
    let mut full = vec!["ledger", "add"];
    full.extend_from_slice(args);
    env.ok(&full)
}

#[test]
fn add_list_modify_remove_round_trip() {
    let env = TestEnv::fresh();

    let entry = add(
        &env,
        &[
            "--amount",
            "30.50",
            "--account",
            "微信",
            "--category",
            "午餐",
            "--date",
            "2026-09-08",
            "--note",
            "小面",
        ],
    );
    assert!(entry["id"].as_str().unwrap().starts_with("ledger-"));
    assert_eq!(entry["amountCents"], 3050, "元要换成整数分存");
    assert_eq!(entry["signed"], "-30.50", "支出对外是带符号的元");
    assert_eq!(entry["accountName"], "微信", "名字回显给 CLI/Agent 读");
    assert_eq!(entry["categoryParentName"], "餐饮", "子分类要能回显大类");

    let income = add(
        &env,
        &[
            "--kind",
            "income",
            "--amount",
            "18155",
            "--account",
            "储蓄卡",
            "--category",
            "工资薪金",
            "--date",
            "2026-09-08",
        ],
    );
    assert_eq!(income["signed"], "18155.00", "core 的 signed 只带负号，正号是前端展示层加的");

    let listed = env.ok(&["ledger", "list", "--date", "2026-09-08"]);
    assert_eq!(listed["total"], 2);

    // 金额第三位小数四舍五入到分
    let rounded = add(
        &env,
        &["--amount", "1.005", "--account", "现金", "--date", "2026-09-07"],
    );
    assert_eq!(rounded["amountCents"], 101);

    // 修改只动给了的字段
    let id = entry["id"].as_str().unwrap();
    let modified = env.ok(&["ledger", "modify", "--id", id, "--amount", "35", "--note", "加了一份小菜"]);
    assert_eq!(modified["amountCents"], 3500);
    assert_eq!(modified["note"], "加了一份小菜");
    assert_eq!(modified["categoryId"], entry["categoryId"], "没给的字段不动");

    // 删除要确认门
    env.err(&["ledger", "remove", "--id", id], 10);
    let removed = env.ok(&["ledger", "remove", "--id", id, "--yes"]);
    assert_eq!(removed["removed"], 1);
    let after = env.ok(&["ledger", "list"]);
    assert_eq!(after["total"], 2, "删掉一笔剩两笔");
}

#[test]
fn transfer_moves_balance_without_touching_income_expense() {
    let env = TestEnv::fresh();

    // 用名字转账（CLI 认名字也认 id）
    env.ok(&[
        "ledger", "account-add", "--name", "零钱", "--kind", "cash", "--initial", "100",
    ]);
    env.ok(&[
        "ledger",
        "transfer",
        "--from",
        "零钱",
        "--to",
        "微信",
        "--amount",
        "40",
        "--date",
        "2026-09-08",
    ]);

    let balance = env.ok(&["ledger", "balance"]);
    let per: Vec<(String, i64)> = balance["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["name"].as_str().unwrap().to_string(),
                item["balanceCents"].as_i64().unwrap(),
            )
        })
        .collect();
    let zero = per.iter().find(|(name, _)| name == "零钱").unwrap().1;
    let wechat = per.iter().find(|(name, _)| name == "微信").unwrap().1;
    assert_eq!(zero, 6000, "转出方减 40 元");
    assert_eq!(wechat, 4000, "转入方加 40 元");
    assert_eq!(balance["netCents"], 10000, "转账不改变净资产");

    let stats = env.ok(&["ledger", "stats", "--month", "2026-09"]);
    assert_eq!(stats["totals"]["incomeCents"], 0, "转账不计收入");
    assert_eq!(stats["totals"]["expenseCents"], 0, "转账不计支出");
    assert_eq!(stats["totals"]["transferCents"], 4000);

    // 转出转入不能是同一个账户
    env.err(
        &[
            "ledger", "transfer", "--from", "零钱", "--to", "零钱", "--amount", "1",
        ],
        2,
    );
}

#[test]
fn stats_breaks_down_by_parent_category_and_range() {
    let env = TestEnv::fresh();

    add(&env, &["--amount", "30", "--account", "微信", "--category", "午餐", "--date", "2026-09-08"]);
    add(&env, &["--amount", "20", "--account", "微信", "--category", "晚餐", "--date", "2026-09-08"]);
    add(&env, &["--amount", "40", "--account", "微信", "--category", "公交地铁", "--date", "2026-09-09"]);
    add(&env, &["--amount", "7", "--account", "微信", "--category", "午餐", "--date", "2026-08-09"]);

    let month = env.ok(&["ledger", "stats", "--month", "2026-09"]);
    assert_eq!(month["totals"]["expenseCents"], 9000);
    let categories = month["categories"].as_array().unwrap();
    assert_eq!(categories.len(), 2, "子分类金额并进大类：餐饮 + 交通");
    assert_eq!(categories[0]["name"], "餐饮", "金额大的排前面");
    assert_eq!(categories[0]["cents"], 5000);
    assert_eq!(categories[0]["percent"], 55.56);

    let series = month["series"].as_array().unwrap();
    assert_eq!(series.len(), 30, "月视图逐天补齐空档");
    assert_eq!(series[7]["expenseCents"], 5000, "9 月 8 日两笔合计");
    assert_eq!(series[8]["expenseCents"], 4000, "9 月 9 日一笔");

    let year = env.ok(&["ledger", "stats", "--year", "2026"]);
    assert_eq!(year["totals"]["expenseCents"], 9700, "年视图含 8 月那一笔");
    assert_eq!(year["series"].as_array().unwrap().len(), 12);

    let ranged = env.ok(&["ledger", "stats", "--from", "2026-09-09", "--to", "2026-09-09"]);
    assert_eq!(ranged["totals"]["expenseCents"], 4000);
}

#[test]
fn accounts_and_categories_management_rules() {
    let env = TestEnv::fresh();

    // 种子分类在：支出 10 个大类、收入 6 个大类
    let expense = env.ok(&["ledger", "categories", "--side", "expense"]);
    let parents = expense["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["parentId"].is_null())
        .count();
    assert_eq!(parents, 10);

    // 加子分类挂到大类下
    let added = env.ok(&[
        "ledger", "category-add", "--name", "咖啡", "--parent", "餐饮", "--icon", "Coffee",
    ]);
    assert!(added["parentId"].as_str().is_some());

    // 两级封顶：子分类不能再挂子分类
    let child_id = added["id"].as_str().unwrap();
    env.err(
        &[
            "ledger", "category-add", "--name", "手冲", "--parent", child_id,
        ],
        2,
    );

    // 名下有账的账户不让删
    add(&env, &["--amount", "5", "--account", "现金", "--date", "2026-09-08"]);
    let accounts = env.ok(&["ledger", "accounts"]);
    let cash_id = accounts["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "现金")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let blocked = env.err(&["ledger", "account-remove", "--id", &cash_id, "--yes"], 2);
    assert_eq!(blocked["code"], "LEDGER_ACCOUNT_IN_USE");

    // 删大类连带子分类，名下账目变未分类
    let dining = expense["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "餐饮" && item["parentId"].is_null())
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    env.ok(&["ledger", "category-remove", "--id", &dining, "--yes"]);
    let after = env.ok(&["ledger", "categories", "--side", "expense"]);
    assert!(
        after["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["name"] != "咖啡" && item["name"] != "餐饮"),
        "大类和它的子分类一起消失"
    );
}

#[test]
fn excel_export_import_round_trip() {
    let env = TestEnv::fresh();

    add(&env, &["--amount", "30.50", "--account", "微信", "--category", "午餐", "--date", "2026-09-08", "--note", "小面"]);
    add(&env, &["--kind", "income", "--amount", "18155", "--account", "储蓄卡", "--category", "工资薪金", "--date", "2026-09-08"]);
    env.ok(&["ledger", "account-add", "--name", "零钱", "--kind", "cash", "--initial", "100"]);
    env.ok(&["ledger", "transfer", "--from", "零钱", "--to", "微信", "--amount", "40", "--date", "2026-09-09"]);

    let out = env.path().join("ledger.zip");
    let exported = env.ok(&["ledger", "export", "--out", &out.to_string_lossy()]);
    assert_eq!(exported["entries"], 3);
    assert!(out.exists(), "导出包落盘");

    // 换一个空数据目录导入：账户/分类按名字合并，缺的自动建
    let target = TestEnv::fresh();
    let imported = target.ok(&["ledger", "import", "--file", &out.to_string_lossy(), "--yes"]);
    assert_eq!(imported["imported"], 3);
    assert_eq!(imported["skipped"], 0);

    let listed = target.ok(&["ledger", "list"]);
    assert_eq!(listed["total"], 3);
    let names: Vec<String> = listed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["accountName"].as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"零钱".to_string()), "导出包里的账户跟着进来");

    let balance = target.ok(&["ledger", "balance"]);
    assert_eq!(balance["netCents"], 10000 + 1815500 - 3050, "期初与流水一起还原");

    // 按日期范围导出：只带这一段账目
    let ranged = env.path().join("ranged.zip");
    let exported = env.ok(&[
        "ledger", "export", "--out", &ranged.to_string_lossy(), "--from", "2026-09-09", "--to", "2026-09-09",
    ]);
    assert_eq!(exported["entries"], 1, "范围导出只带 9 月 9 日的转账");

    // 导入要确认门（重复导入会产生重复账目）
    env.err(&["ledger", "import", "--file", &out.to_string_lossy()], 10);
}
