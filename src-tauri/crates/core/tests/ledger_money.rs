#![recursion_limit = "512"]
//! 记账主功能逻辑正确性（v0.7.4 需求 13）：余额 = 期初 + 流水推导（含转账），
//! 统计 = 整数分的精确合计；增/改/删/转账/改转账之后两边必须始终一致、不留残留。
//!
//! 全部走真实命令层（CLI 入参 → core_dispatch），断言的是 balance / accounts / stats
//! 三个只读口径的输出，而不是内部实现——它们同时也是前端与 Agent 读的契约。

mod common;

use common::TestEnv;
use serde_json::Value;

/// 记账写操作全部有确认门（v0.7.2）：测试里的写都视为已获用户同意。
fn add(env: &TestEnv, args: &[&str]) -> Value {
    let mut full = vec!["ledger", "add"];
    full.extend_from_slice(args);
    full.push("--yes");
    env.ok(&full)
}

fn balance_cents(env: &TestEnv, account: &str) -> i64 {
    let balance = env.ok(&["ledger", "balance"]);
    balance["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"].as_str() == Some(account))
        .unwrap_or_else(|| panic!("账户 {account} 不在 balance 输出里"))["balanceCents"]
        .as_i64()
        .unwrap()
}

fn stats(env: &TestEnv, month: &str) -> Value {
    env.ok(&["ledger", "stats", "--month", month])
}

/// a) 记一笔支出：付款账户余额减少额正确，balance 的净资产与 stats 的总支出正确。
#[test]
fn expense_moves_balance_and_stats() {
    let env = TestEnv::fresh();
    env.ok(&["ledger", "account-modify", "--id", "lacc-01", "--initial", "500", "--yes"]);

    let entry = add(
        &env,
        &[
            "--amount", "30.50",
            "--account", "现金",
            "--category", "午餐",
            "--date", "2026-09-08",
        ],
    );
    assert_eq!(entry["amountCents"], 3050);
    assert_eq!(balance_cents(&env, "现金"), 50000 - 3050, "余额 = 期初 − 支出");

    let balance = env.ok(&["ledger", "balance"]);
    assert_eq!(balance["netCents"], 50000 - 3050, "其它账户都是 0，净资产跟着走");
    assert_eq!(balance["liabilitiesCents"], 0, "非 credit 账户的负余额不算负债");

    let stats = stats(&env, "2026-09");
    assert_eq!(stats["totals"]["expenseCents"], 3050);
    assert_eq!(stats["totals"]["incomeCents"], 0);
    assert_eq!(stats["totals"]["netCents"], -3050);
}

/// b) 改金额是**替换**不是叠加：余额与统计都只反映新值。
#[test]
fn modify_amount_replaces_not_stacks() {
    let env = TestEnv::fresh();
    let entry = add(
        &env,
        &["--amount", "100", "--account", "现金", "--category", "午餐", "--date", "2026-09-08"],
    );
    let id = entry["id"].as_str().unwrap().to_string();

    let modified = env.ok(&["ledger", "modify", "--id", &id, "--amount", "150", "--yes"]);
    assert_eq!(modified["amountCents"], 15000);
    assert_eq!(balance_cents(&env, "现金"), -15000, "改金额后余额只减一次新值");
    assert_eq!(stats(&env, "2026-09")["totals"]["expenseCents"], 15000);

    env.ok(&["ledger", "modify", "--id", &id, "--amount", "80", "--yes"]);
    assert_eq!(balance_cents(&env, "现金"), -8000, "再改仍是替换");
    assert_eq!(stats(&env, "2026-09")["totals"]["expenseCents"], 8000);
}

/// c) 支出改成收入：两侧统计与余额同时翻转；不改分类直接翻类型要被拒
/// （支出分类不能挂在收入上——core 的 SIDE_MISMATCH 护栏）。
#[test]
fn modify_kind_expense_to_income_flips_both_sides() {
    let env = TestEnv::fresh();
    let entry = add(
        &env,
        &["--amount", "200", "--account", "现金", "--category", "午餐", "--date", "2026-09-08"],
    );
    let id = entry["id"].as_str().unwrap().to_string();

    let error = env.err(&["ledger", "modify", "--id", &id, "--kind", "income", "--yes"], 2);
    assert_eq!(error["code"], "LEDGER_CATEGORY_SIDE_MISMATCH");

    let modified = env.ok(&[
        "ledger", "modify", "--id", &id,
        "--kind", "income", "--category", "工资薪金", "--yes",
    ]);
    assert_eq!(modified["kind"], "income");
    assert_eq!(modified["signed"], "200.00", "收入是正向");
    assert_eq!(balance_cents(&env, "现金"), 20000, "余额从 −200 翻到 +200");

    let stats = stats(&env, "2026-09");
    assert_eq!(stats["totals"]["incomeCents"], 20000);
    assert_eq!(stats["totals"]["expenseCents"], 0, "支出侧不能残留");
}

/// d) 删除一笔：余额与统计都回到之前的状态。
#[test]
fn remove_restores_balance_and_stats() {
    let env = TestEnv::fresh();
    env.ok(&["ledger", "account-modify", "--id", "lacc-01", "--initial", "500", "--yes"]);
    let gone = add(
        &env,
        &["--amount", "120", "--account", "现金", "--category", "晚餐", "--date", "2026-09-08"],
    );
    let kept = add(
        &env,
        &["--amount", "30", "--account", "现金", "--category", "午餐", "--date", "2026-09-08"],
    );
    let gone_id = gone["id"].as_str().unwrap().to_string();
    assert_eq!(balance_cents(&env, "现金"), 50000 - 12000 - 3000);

    env.ok(&["ledger", "remove", "--id", &gone_id, "--yes"]);
    assert_eq!(balance_cents(&env, "现金"), 50000 - 3000, "删掉的那笔不再扣钱");
    let stats = stats(&env, "2026-09");
    assert_eq!(stats["totals"]["expenseCents"], 3000);
    assert_eq!(stats["count"], 1);
    assert_eq!(env.ok(&["ledger", "get", "--id", kept["id"].as_str().unwrap()])["amountCents"], 3000);
}

/// e) 转账：转出账户减、转入账户加，收支统计不含转账（transferCents 单列）。
#[test]
fn transfer_moves_money_and_stays_out_of_income_expense() {
    let env = TestEnv::fresh();
    env.ok(&["ledger", "account-modify", "--id", "lacc-04", "--initial", "1000", "--yes"]);

    env.ok(&[
        "ledger", "transfer",
        "--from", "储蓄卡", "--to", "微信", "--amount", "250", "--date", "2026-09-08", "--yes",
    ]);
    assert_eq!(balance_cents(&env, "储蓄卡"), 100000 - 25000);
    assert_eq!(balance_cents(&env, "微信"), 25000);

    let balance = env.ok(&["ledger", "balance"]);
    assert_eq!(balance["netCents"], 100000, "转账不改变净资产");

    let stats = stats(&env, "2026-09");
    assert_eq!(stats["totals"]["incomeCents"], 0, "转账不是收入");
    assert_eq!(stats["totals"]["expenseCents"], 0, "转账不是支出");
    assert_eq!(stats["totals"]["transferCents"], 25000);
    assert!(
        stats["categories"].as_array().unwrap().is_empty(),
        "分类占比里也不该出现转账"
    );
}

/// f) 改一笔转账（金额 / 转入账户）：旧账户关系**完全撤销**、新关系生效，不留残留。
#[test]
fn modify_transfer_reroutes_without_residue() {
    let env = TestEnv::fresh();
    env.ok(&["ledger", "account-modify", "--id", "lacc-04", "--initial", "1000", "--yes"]);
    let transfer = env.ok(&[
        "ledger", "transfer",
        "--from", "储蓄卡", "--to", "微信", "--amount", "250", "--date", "2026-09-08", "--yes",
    ]);
    let id = transfer["id"].as_str().unwrap().to_string();

    // 改转入账户：微信回到 0，支付宝接手 +250
    env.ok(&["ledger", "modify", "--id", &id, "--to", "支付宝", "--yes"]);
    assert_eq!(balance_cents(&env, "储蓄卡"), 75000);
    assert_eq!(balance_cents(&env, "微信"), 0, "旧转入账户不能残留");
    assert_eq!(balance_cents(&env, "支付宝"), 25000);

    // 改金额：两侧同时按新金额生效
    env.ok(&["ledger", "modify", "--id", &id, "--amount", "100", "--yes"]);
    assert_eq!(balance_cents(&env, "储蓄卡"), 90000);
    assert_eq!(balance_cents(&env, "支付宝"), 10000);
    assert_eq!(balance_cents(&env, "微信"), 0);

    let balance = env.ok(&["ledger", "balance"]);
    assert_eq!(balance["netCents"], 100000, "净资产始终不变");
    assert_eq!(stats(&env, "2026-09")["totals"]["transferCents"], 10000);
}

/// g) stats 的分类合计 = 该分类（含并入大类的子分类）下 entries 之和——整数分口径，
/// 各分组之和恰好等于总支出，没有浮点 drift。
#[test]
fn stats_category_totals_equal_entry_sums() {
    let env = TestEnv::fresh();
    add(&env, &["--amount", "10.50", "--account", "现金", "--category", "午餐", "--date", "2026-09-01"]);
    add(&env, &["--amount", "20", "--account", "现金", "--category", "晚餐", "--date", "2026-09-02"]);
    add(&env, &["--amount", "7", "--account", "现金", "--category", "饮料", "--date", "2026-09-03"]);
    add(&env, &["--amount", "30", "--account", "现金", "--category", "打车", "--date", "2026-09-03"]);

    let stats = stats(&env, "2026-09");
    let categories = stats["categories"].as_array().unwrap().clone();
    let group = |name: &str| {
        categories
            .iter()
            .find(|item| item["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("stats 里没有分类 {name}"))
            .clone()
    };
    // 子分类金额并进大类：午餐 1050 + 晚餐 2000 + 饮料 700
    assert_eq!(group("餐饮")["cents"], 3750);
    assert_eq!(group("餐饮")["count"], 3);
    assert_eq!(group("交通")["cents"], 3000);
    assert_eq!(group("交通")["count"], 1);

    let expense_total = stats["totals"]["expenseCents"].as_i64().unwrap();
    assert_eq!(expense_total, 6750);
    let group_sum: i64 = categories
        .iter()
        .filter(|item| item["side"].as_str() == Some("expense"))
        .map(|item| item["cents"].as_i64().unwrap())
        .sum();
    assert_eq!(group_sum, expense_total, "分组之和 = 总支出（整数分，无 drift）");
    assert_eq!(group("餐饮")["percent"], 55.56, "百分比按分精确计算");
}

/// h) 账户余额 = 期初 + 流水推导（收入 + / 支出 − / 转账两侧），
/// accounts 与 balance 两个只读口径输出一致。
#[test]
fn balance_equals_initial_plus_flows() {
    let env = TestEnv::fresh();
    env.ok(&["ledger", "account-modify", "--id", "lacc-02", "--initial", "88.88", "--yes"]);

    add(
        &env,
        &["--kind", "income", "--amount", "1000", "--account", "微信", "--category", "工资薪金", "--date", "2026-09-01"],
    );
    add(
        &env,
        &["--amount", "250.50", "--account", "微信", "--category", "午餐", "--date", "2026-09-02"],
    );
    env.ok(&[
        "ledger", "transfer", "--from", "微信", "--to", "现金", "--amount", "100", "--date", "2026-09-03", "--yes",
    ]);

    let expected = 8888 + 100000 - 25050 - 10000;
    assert_eq!(balance_cents(&env, "微信"), expected);
    assert_eq!(balance_cents(&env, "现金"), 10000);

    // accounts 与 balance 必须同口径
    let accounts = env.ok(&["ledger", "accounts"]);
    let wechat = accounts["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"].as_str() == Some("微信"))
        .unwrap();
    assert_eq!(wechat["balanceCents"], expected);
    assert_eq!(accounts["netCents"], expected + 10000);
    let balance = env.ok(&["ledger", "balance"]);
    assert_eq!(balance["netCents"], accounts["netCents"]);
    assert_eq!(balance["net"], accounts["net"]);
}

/// 账户类型放开为自由字符串（v0.7.4）：任意非空串合法、原样存取；
/// 唯一保留的语义是 credit（信用卡）——负余额计入总负债；空串被拒。
#[test]
fn account_kind_is_free_form_and_credit_drives_liabilities() {
    let env = TestEnv::fresh();

    let fund = env.ok(&["ledger", "account-add", "--name", "公积金", "--kind", "公积金", "--yes"]);
    assert_eq!(fund["kind"], "公积金", "自由字符串原样回显");
    assert_eq!(fund["kind"], env.ok(&["ledger", "accounts"])["items"]
        .as_array().unwrap().iter()
        .find(|item| item["name"].as_str() == Some("公积金")).unwrap()["kind"]);

    let error = env.err(&["ledger", "account-add", "--name", "空类型", "--kind", "   ", "--yes"], 2);
    assert_eq!(error["code"], "LEDGER_ACCOUNT_KIND_INVALID");

    // 改成另一个自由类型
    let modified = env.ok(&["ledger", "account-modify", "--id", fund["id"].as_str().unwrap(), "--kind", "医保", "--yes"]);
    assert_eq!(modified["kind"], "医保");

    // credit 的负债口径不因放开而丢
    env.ok(&["ledger", "account-add", "--name", "信用卡", "--kind", "credit", "--yes"]);
    add(
        &env,
        &["--amount", "500", "--account", "信用卡", "--category", "午餐", "--date", "2026-09-08"],
    );
    let balance = env.ok(&["ledger", "balance"]);
    assert_eq!(balance_cents(&env, "信用卡"), -50000);
    assert_eq!(balance["liabilitiesCents"], 50000, "credit 负余额计入负债");
    assert_eq!(balance["netCents"], -50000);
    assert_eq!(balance["assetsCents"], 0, "总资产 = 净资产 + 负债");
}

/// 条目的附图字段（v0.7.4）：add/modify 存取、空串清除、危险文件名被拒。
#[test]
fn entry_image_field_round_trip() {
    let env = TestEnv::fresh();
    let entry = add(
        &env,
        &["--amount", "12", "--account", "现金", "--date", "2026-09-08", "--image", "md-1-1.png"],
    );
    assert_eq!(entry["image"], "md-1-1.png");
    let id = entry["id"].as_str().unwrap().to_string();
    assert_eq!(env.ok(&["ledger", "get", "--id", &id])["image"], "md-1-1.png");

    // 落盘的 JSON 里就是裸文件名（同步 payload 同源）
    let file = env.read_file("ledger.json");
    assert_eq!(file["entries"][0]["image"], "md-1-1.png");

    // 不带 --image 的 modify 保留原图
    let touched = env.ok(&["ledger", "modify", "--id", &id, "--note", "换了备注", "--yes"]);
    assert_eq!(touched["image"], "md-1-1.png");

    // 空串 = 清除
    let cleared = env.ok(&["ledger", "modify", "--id", &id, "--image", "", "--yes"]);
    assert!(cleared["image"].is_null());

    // 路径穿越等危险名字在写入口就被拒
    let error = env.err(
        &["ledger", "add", "--amount", "1", "--account", "现金", "--image", "../evil.png", "--yes"],
        2,
    );
    assert_eq!(error["code"], "LEDGER_IMAGE_NAME_INVALID");
}

/// icon-list 输出双目录：分类口径（total/groups/icons）不动，账户目录另给一份。
#[test]
fn icon_list_reports_both_catalogs() {
    let env = TestEnv::fresh();
    let list = env.ok(&["ledger", "icon-list"]);
    let icons_len = list["icons"].as_array().unwrap().len() as u64;
    assert_eq!(list["total"].as_u64().unwrap(), icons_len);
    assert!(list["total"].as_u64().unwrap() >= 200, "分类目录口径不变");
    let account_icons = list["accountIcons"].as_array().unwrap();
    assert_eq!(list["accountTotal"].as_u64().unwrap(), account_icons.len() as u64);
    assert!(list["accountTotal"].as_u64().unwrap() >= 30);
    let groups = list["accountGroups"].as_array().unwrap();
    assert_eq!(groups.len(), 6);
    assert_eq!(groups[0]["name"], "现金与卡");
    // 种子账户的四个图标都在账户目录里
    for icon in ["Wallet", "MessageCircle", "Smartphone", "Landmark"] {
        assert!(
            account_icons.iter().any(|item| item.as_str() == Some(icon)),
            "账户目录缺 {icon}"
        );
    }
}
