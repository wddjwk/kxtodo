#![recursion_limit = "512"]
//! ledger domain end-to-end tests：记账的增删改查、转账与余额、统计、Excel 导出导入往返。

mod common;

use common::TestEnv;
use serde_json::Value;

fn add(env: &TestEnv, args: &[&str]) -> Value {
    // 记账写操作全部有确认门（v0.7.2）：测试里的写都视为已获用户同意
    let mut full = vec!["ledger", "add"];
    full.extend_from_slice(args);
    full.push("--yes");
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
    let modified = env.ok(&["ledger", "modify", "--id", id, "--amount", "35", "--note", "加了一份小菜", "--yes"]);
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
        "ledger", "account-add", "--name", "零钱", "--kind", "cash", "--initial", "100", "--yes",
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
        "--yes",
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

    // 转出转入不能是同一个账户（带上 --yes 才走到校验，确认门先于一切拦截）
    env.err(
        &[
            "ledger", "transfer", "--from", "零钱", "--to", "零钱", "--amount", "1", "--yes",
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
        "ledger", "category-add", "--name", "咖啡", "--parent", "餐饮", "--icon", "Coffee", "--yes",
    ]);
    assert!(added["parentId"].as_str().is_some());

    // 两级封顶：子分类不能再挂子分类（带 --yes 才走到校验）
    let child_id = added["id"].as_str().unwrap();
    env.err(
        &[
            "ledger", "category-add", "--name", "手冲", "--parent", child_id, "--yes",
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
    env.ok(&["ledger", "account-add", "--name", "零钱", "--kind", "cash", "--initial", "100", "--yes"]);
    env.ok(&["ledger", "transfer", "--from", "零钱", "--to", "微信", "--amount", "40", "--date", "2026-09-09", "--yes"]);

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

#[test]
fn ledger_writes_without_yes_are_gated_and_touch_nothing() {
    let env = TestEnv::fresh();

    // 记一笔不带 --yes：确认门拦下（退出码 10 / CONFIRMATION_REQUIRED），账本分毫未动
    let error = env.err(&["ledger", "add", "--amount", "12", "--account", "现金"], 10);
    assert_eq!(error["code"], "CONFIRMATION_REQUIRED");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("记账数据敏感"), "确认文案要告诉 Agent 先经用户同意：{message}");
    assert!(message.contains("--yes"), "确认文案要指路 --yes：{message}");
    let listed = env.ok(&["ledger", "list"]);
    assert_eq!(listed["total"], 0, "被拦下的写不落账");

    // 其余写动作同样被拦
    env.err(&["ledger", "account-add", "--name", "零钱"], 10);
    env.err(&["ledger", "category-add", "--name", "咖啡"], 10);
    env.err(&["ledger", "transfer", "--from", "现金", "--to", "微信", "--amount", "1"], 10);
    env.err(&["ledger", "account-type-add", "--name", "公积金"], 10);
    env.err(&["ledger", "account-type-modify", "--id", "latype-xxxx", "--name", "医保"], 10);
    env.err(&["ledger", "account-type-remove", "--id", "latype-xxxx"], 10);

    // 带上 --yes 才落账
    add(&env, &["--amount", "12", "--account", "现金"]);
    let listed = env.ok(&["ledger", "list"]);
    assert_eq!(listed["total"], 1);

    // modify 也被拦；拦下的修改不生效
    let id = listed["items"].as_array().unwrap()[0]["id"].as_str().unwrap();
    env.err(&["ledger", "modify", "--id", id, "--amount", "99"], 10);
    let got = env.ok(&["ledger", "get", "--id", id]);
    assert_eq!(got["amountCents"], 1200, "被拦下的修改不动数据");

    // 读操作永远不需要确认
    env.ok(&["ledger", "accounts"]);
    env.ok(&["ledger", "categories"]);
    env.ok(&["ledger", "stats"]);
    env.ok(&["ledger", "balance"]);
    env.ok(&["ledger", "account-types"]);
}

/// 自定义账户类型（v0.7.5）：增/改/删走 ops 层的完整往返；删掉仍被账户引用的类型
/// 是允许的（账户 kind 是自由字符串，原样保留）；名字唯一；未知 id 报 NOT_FOUND。
#[test]
fn account_type_crud_round_trip() {
    let env = TestEnv::fresh();
    assert_eq!(env.ok(&["ledger", "account-types"])["total"], 0, "首跑没有自定义类型");

    let created = env.ok(&[
        "ledger", "account-type-add", "--name", "公积金", "--icon", "PiggyBank", "--color", "#7f8fa6", "--yes",
    ]);
    let id = created["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("latype-"), "类型 id 用 latype- 前缀：{id}");
    assert_eq!(created["name"], "公积金");
    assert_eq!(created["icon"], "PiggyBank");
    assert_eq!(created["color"], "#7f8fa6");

    // 落盘进 ledger.json 的 accountTypes（同步实体同源）
    let file = env.read_file("ledger.json");
    assert_eq!(file["accountTypes"][0]["id"], serde_json::json!(id));

    // 重名被拒；空名被拒
    let dup = env.err(&["ledger", "account-type-add", "--name", "公积金", "--yes"], 2);
    assert_eq!(dup["code"], "LEDGER_ACCOUNT_TYPE_EXISTS");
    let empty = env.err(&["ledger", "account-type-add", "--name", "  ", "--yes"], 2);
    assert_eq!(empty["code"], "LEDGER_ACCOUNT_TYPE_NAME_EMPTY");

    // 改名/改图标
    let modified = env.ok(&[
        "ledger", "account-type-modify", "--id", &id, "--name", "医保", "--icon", "HeartPulse", "--yes",
    ]);
    assert_eq!(modified["name"], "医保");
    assert_eq!(modified["icon"], "HeartPulse");
    assert_eq!(modified["color"], "#7f8fa6", "没给的字段不动");
    assert!(modified["updatedAt"].is_string(), "修改要抬 updatedAt（同步 LWW 的比较键）");

    // 账户在用也允许删：kind 字符串原样保留
    let account = env.ok(&["ledger", "account-add", "--name", "医保账户", "--kind", "医保", "--yes"]);
    env.ok(&["ledger", "account-type-remove", "--id", &id, "--yes"]);
    assert_eq!(env.ok(&["ledger", "account-types"])["total"], 0);
    let accounts = env.ok(&["ledger", "accounts"]);
    let kept = accounts["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"].as_str() == Some(account["id"].as_str().unwrap()))
        .unwrap();
    assert_eq!(kept["kind"], "医保", "删类型不动账户的 kind");

    // 删除写同步墓碑
    let file = env.read_file("ledger.json");
    let tombstoned = file["_meta"]["tombstones"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tomb| tomb["id"].as_str() == Some(id.as_str()) && tomb["type"] == "ledgerAccountType");
    assert!(tombstoned, "删除类型要写 ledgerAccountType 墓碑");

    // 未知 id
    let missing = env.err(&["ledger", "account-type-modify", "--id", "latype-nope", "--name", "x", "--yes"], 2);
    assert_eq!(missing["code"], "LEDGER_ACCOUNT_TYPE_NOT_FOUND");
    let missing = env.err(&["ledger", "account-type-remove", "--id", "latype-nope", "--yes"], 2);
    assert_eq!(missing["code"], "LEDGER_ACCOUNT_TYPE_NOT_FOUND");
}

/// `series[*].key` 的横轴（用来断言 day/month 粒度到底枚举了哪些桶）。
fn series_keys(value: &Value) -> Vec<&str> {
    value["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["key"].as_str().unwrap())
        .collect()
}

#[test]
fn stats_series_follows_from_to_instead_of_the_whole_month() {
    let env = TestEnv::fresh();
    add(&env, &["--amount", "10", "--account", "微信", "--category", "午餐", "--date", "2026-09-15"]);
    add(&env, &["--amount", "20", "--account", "微信", "--category", "晚餐", "--date", "2026-09-29"]);
    add(&env, &["--amount", "30", "--account", "微信", "--category", "午餐", "--date", "2026-10-01"]);

    // day 粒度的横轴就是 --from..--to（含两端），不再是 from 所在的整个自然月：
    // 做周报的调用方不必自己裁，也不会把整月数据当成一周用。
    let week = env.ok(&["ledger", "stats", "--from", "2026-09-14", "--to", "2026-09-21"]);
    assert_eq!(week["range"]["grain"], "day");
    assert_eq!(
        series_keys(&week),
        vec![
            "2026-09-14", "2026-09-15", "2026-09-16", "2026-09-17",
            "2026-09-18", "2026-09-19", "2026-09-20", "2026-09-21",
        ]
    );
    assert_eq!(week["series"][1]["expenseCents"], 1000, "09-15 那一笔在区间里");
    assert_eq!(week["totals"]["expenseCents"], 1000, "合计只算区间内的");

    // 跨月的短区间同样逐日枚举（周报常常横跨月底）
    let crossing = env.ok(&["ledger", "stats", "--from", "2026-09-28", "--to", "2026-10-03"]);
    assert_eq!(crossing["range"]["grain"], "day", "门槛内就按天，哪怕跨月");
    assert_eq!(
        series_keys(&crossing),
        vec![
            "2026-09-28", "2026-09-29", "2026-09-30",
            "2026-10-01", "2026-10-02", "2026-10-03",
        ]
    );
    assert_eq!(crossing["series"][1]["expenseCents"], 2000);
    assert_eq!(crossing["series"][3]["expenseCents"], 3000);

    // --month 的行为一点没变：整月逐日补齐空档
    let month = env.ok(&["ledger", "stats", "--month", "2026-09"]);
    assert_eq!(month["range"]["grain"], "day");
    assert_eq!(series_keys(&month).len(), 30);
    assert_eq!(series_keys(&month)[0], "2026-09-01");
    assert_eq!(series_keys(&month)[29], "2026-09-30");
    assert_eq!(month["series"][14]["expenseCents"], 1000, "09-15");
    assert_eq!(month["series"][28]["expenseCents"], 2000, "09-29");

    // 更长的区间仍按月逐月枚举
    let year = env.ok(&["ledger", "stats", "--year", "2026"]);
    assert_eq!(year["range"]["grain"], "month");
    assert_eq!(series_keys(&year).len(), 12);
    let long = env.ok(&["ledger", "stats", "--from", "2026-08-01", "--to", "2026-10-31"]);
    assert_eq!(series_keys(&long), vec!["2026-08", "2026-09", "2026-10"]);

    // 日粒度门槛含两端 62 天（与 GUI 的 bucketOf 同一个数，core 侧另有测试钉住）：
    // 62 天逐日、63 天转月。32~62 天这一段在 v0.8.0 之前两边结论相反。
    let two_months = env.ok(&["ledger", "stats", "--from", "2026-09-01", "--to", "2026-11-01"]);
    assert_eq!(two_months["range"]["grain"], "day");
    assert_eq!(series_keys(&two_months).len(), 62);
    let over = env.ok(&["ledger", "stats", "--from", "2026-09-01", "--to", "2026-11-02"]);
    assert_eq!(over["range"]["grain"], "month");
    assert_eq!(series_keys(&over), vec!["2026-09", "2026-10", "2026-11"]);

    // 开区间（不给 --from/--to）按账目里出现过的桶收表，不枚举几百万个空档
    assert_eq!(series_keys(&env.ok(&["ledger", "stats"])), vec!["2026-09", "2026-10"]);

    // 闰月与倒挂区间
    assert_eq!(series_keys(&env.ok(&["ledger", "stats", "--month", "2028-02"])).len(), 29);
    assert_eq!(
        series_keys(&env.ok(&["ledger", "stats", "--from", "2026-09-20", "--to", "2026-09-10"])),
        Vec::<&str>::new(),
        "倒挂的区间里没有任何一天"
    );
}

#[test]
fn stats_series_and_category_shape_is_locked() {
    // stats 的聚合从「每个 key 重扫全部 entries + 在 Value 数组里线性找槽位」改成
    // 一次遍历 + id 索引，输出必须逐字节等价：这里把 series 与 categories 的**序列化
    // 文本**钉住（字段顺序也算——serde_json 开了 preserve_order）。
    let env = TestEnv::fresh();
    add(&env, &["--amount", "30", "--account", "微信", "--category", "午餐", "--date", "2026-09-08"]);
    add(&env, &["--amount", "20", "--account", "微信", "--category", "晚餐", "--date", "2026-09-08"]);
    add(&env, &["--amount", "40", "--account", "微信", "--category", "公交地铁", "--date", "2026-09-09"]);
    add(&env, &["--kind", "income", "--amount", "100", "--account", "储蓄卡", "--category", "工资薪金", "--date", "2026-09-10"]);

    let stats = env.ok(&["ledger", "stats", "--from", "2026-09-08", "--to", "2026-09-10"]);
    assert_eq!(
        stats["series"].to_string(),
        concat!(
            r#"[{"key":"2026-09-08","incomeCents":0,"expenseCents":5000},"#,
            r#"{"key":"2026-09-09","incomeCents":0,"expenseCents":4000},"#,
            r#"{"key":"2026-09-10","incomeCents":10000,"expenseCents":0}]"#
        )
    );
    assert_eq!(
        stats["categories"].to_string(),
        concat!(
            r#"[{"categoryId":"lcat-inc-01","name":"工资","side":"income","count":1,"cents":10000,"percent":100.0},"#,
            r#"{"categoryId":"lcat-exp-01","name":"餐饮","side":"expense","count":2,"cents":5000,"percent":55.56},"#,
            r#"{"categoryId":"lcat-exp-02","name":"交通","side":"expense","count":1,"cents":4000,"percent":44.44}]"#
        ),
        "子分类并进大类、按金额降序、percent 两位小数"
    );
}

#[test]
fn list_returns_everything_by_default_and_double_writes_paging_meta() {
    let env = TestEnv::fresh();
    for day in 1..=5 {
        let date = format!("2026-09-0{day}");
        add(&env, &["--amount", "10", "--account", "微信", "--date", &date]);
    }
    let ids = |envelope: &Value| -> Vec<String> {
        envelope["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["id"].as_str().unwrap().to_string())
            .collect()
    };

    // 不传 --limit 就返回全部：记账是金融数据，静默截断会让「这个月花了多少」算出残值
    let all = env.run(&["ledger", "list"]);
    assert_eq!(all.code, 0, "{}", all.stderr);
    let envelope = all.envelope();
    assert_eq!(envelope["data"]["total"], 5);
    assert_eq!(envelope["data"]["returned"], 5);
    assert_eq!(envelope["data"]["items"].as_array().unwrap().len(), 5);
    // meta 双写同一份分页信息：render 的合计行（--format table/pretty）只认 meta.count
    assert_eq!(envelope["meta"]["count"], 5);
    assert_eq!(envelope["meta"]["nextCursor"], Value::Null);

    // --limit 分页：data.total 仍是过滤后的全部，returned 是这一页，游标进 meta.nextCursor
    let first = env.run(&["ledger", "list", "--limit", "2"]);
    let envelope = first.envelope();
    assert_eq!(envelope["data"]["total"], 5);
    assert_eq!(envelope["data"]["returned"], 2);
    assert_eq!(envelope["meta"]["count"], 5);
    assert_eq!(envelope["meta"]["nextCursor"], "2");

    // --cursor 接着翻，与 task list 同一套语义（偏移量，两页不重叠）
    let first_ids = ids(&first.envelope());
    let second = env.run(&["ledger", "list", "--limit", "2", "--cursor", "2"]);
    let envelope = second.envelope();
    assert_eq!(envelope["meta"]["nextCursor"], "4");
    let second_ids = ids(&envelope);
    assert_eq!(second_ids.len(), 2);
    assert!(
        first_ids.iter().all(|id| !second_ids.contains(id)),
        "第二页不该重复第一页：{first_ids:?} / {second_ids:?}"
    );
    let third = env.run(&["ledger", "list", "--limit", "2", "--cursor", "4"]);
    let envelope = third.envelope();
    assert_eq!(envelope["data"]["returned"], 1, "最后一页只剩一笔");
    assert_eq!(envelope["meta"]["nextCursor"], Value::Null, "没有下一页就不给游标");

    // --all 忽略分页（哪怕同时给了 --limit）
    let forced = env.run(&["ledger", "list", "--limit", "2", "--all"]);
    let envelope = forced.envelope();
    assert_eq!(envelope["data"]["returned"], 5);
    assert_eq!(envelope["meta"]["nextCursor"], Value::Null);

    // 天文数字的 --limit 不许溢出（paginate 用 saturating_add；debug 构建下裸加法会 panic）
    let huge = env.run(&["ledger", "list", "--limit", "18446744073709551615"]);
    assert_eq!(huge.code, 0, "{}", huge.stderr);
    assert_eq!(huge.envelope()["data"]["returned"], 5);

    // 过滤后的 total 只算命中的笔数
    let filtered = env.ok(&["ledger", "list", "--from", "2026-09-01", "--to", "2026-09-03"]);
    assert_eq!(filtered["total"], 3);
    assert_eq!(filtered["returned"], 3);

    // 游标非法 → 退出码 2
    let bad = env.err(&["ledger", "list", "--cursor", "abc"], 2);
    assert_eq!(bad["code"], "INVALID_CURSOR");

    // --format table 的合计行回来了（此前 meta 里没有 count，退化成逐行裸 JSON）
    let table = env.run(&["ledger", "list", "--limit", "2", "--format", "table"]);
    assert_eq!(table.code, 0, "{}", table.stderr);
    assert!(table.stdout.contains("-- 共 5 条"), "{}", table.stdout);
    assert_eq!(table.stdout.lines().count(), 3, "两行条目 + 一行合计");
}
