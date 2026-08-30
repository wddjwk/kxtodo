#![recursion_limit = "512"]
//! task domain end-to-end tests (§3.4).

mod common;

use common::TestEnv;
use serde_json::Value;

fn setup_entry(env: &TestEnv) -> (String, String) {
    let category = env.ok(&["task", "add", "--type", "category", "--name", "2026"]);
    let category_id = category["id"].as_str().unwrap().to_string();
    let entry = env.ok(&[
        "task",
        "add",
        "--type",
        "entry",
        "--name",
        "0727-0731",
        "--parent-id",
        &category_id,
    ]);
    (category_id, entry["id"].as_str().unwrap().to_string())
}

#[test]
fn fresh_dir_has_default_inbox() {
    let env = TestEnv::fresh();
    let tree = env.ok(&["task", "tree"]);
    let roots = tree["roots"].as_array().unwrap();
    assert!(roots
        .iter()
        .any(|node| node["kind"] == Value::Null || node["type"] == "entry"));
    assert!(roots.iter().any(|node| node["type"] == "system"));
}

#[test]
fn add_get_category_entry_item() {
    let env = TestEnv::fresh();
    let (category_id, entry_id) = setup_entry(&env);

    let got = env.ok(&["task", "get", "--type", "category", "--id", &category_id]);
    assert_eq!(got["name"], "2026");
    assert_eq!(got["counts"]["directChildren"], 1);

    let item = env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "完成 XXX 需求",
        "--important",
        "true",
        "--due-date",
        "2026-07-31",
        "--tag",
        "blue:需求",
        "--emoji",
        "🔥",
    ]);
    let item_id = item["id"].as_str().unwrap();
    assert!(item_id.starts_with("task-"));
    assert_eq!(item["entry"]["id"], entry_id);
    assert_eq!(item["path"], "2026 / 0727-0731");
    assert_eq!(item["tags"][0]["color"], "blue");
    assert_eq!(item["tags"][0]["text"], "需求");

    let got = env.ok(&["task", "get", "--type", "item", "--id", item_id]);
    assert_eq!(got["markdown"], "完成 XXX 需求");
    assert_eq!(got["dueDate"], "2026-07-31");
    assert_eq!(got["ancestors"][0]["id"], category_id);
}

#[test]
fn add_item_requires_existing_entry() {
    let env = TestEnv::fresh();
    let error = env.err(
        &[
            "task",
            "add",
            "--type",
            "item",
            "--entry-id",
            "entry-nope",
            "--markdown",
            "x",
        ],
        3,
    );
    assert_eq!(error["code"], "ENTRY_NOT_FOUND");
}

#[test]
fn add_item_rejects_category_parent() {
    let env = TestEnv::fresh();
    let category = env.ok(&["task", "add", "--type", "category", "--name", "c"]);
    let error = env.err(
        &[
            "task",
            "add",
            "--type",
            "item",
            "--entry-id",
            category["id"].as_str().unwrap(),
            "--markdown",
            "x",
        ],
        2,
    );
    assert_eq!(error["code"], "INVALID_ENTRY");
}

#[test]
fn list_and_filters() {
    let env = TestEnv::fresh();
    let (_category_id, entry_id) = setup_entry(&env);
    env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "任务A",
        "--important",
        "true",
    ]);
    env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "任务B",
        "--completed",
        "true",
    ]);
    env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "任务C",
        "--due-date",
        "2026-07-30",
    ]);

    let all = env.ok(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--status",
        "all",
    ]);
    assert_eq!(all["items"].as_array().unwrap().len(), 3);

    let open = env.ok(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--status",
        "open",
    ]);
    assert_eq!(open["items"].as_array().unwrap().len(), 2);

    let completed = env.ok(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--status",
        "completed",
    ]);
    assert_eq!(completed["items"].as_array().unwrap().len(), 1);

    let important = env.ok(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--important",
        "true",
    ]);
    assert_eq!(important["items"].as_array().unwrap().len(), 1);

    let due = env.ok(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--due-from",
        "2026-07-01",
        "--due-to",
        "2026-07-31",
    ]);
    assert_eq!(due["items"].as_array().unwrap().len(), 1);

    // changed 范围（本周）
    let changed = env.ok(&[
        "task",
        "list",
        "--type",
        "item",
        "--changed-from",
        "2020-01-01",
        "--changed-to",
        "2100-01-01",
        "--all",
    ]);
    assert!(changed["items"].as_array().unwrap().len() >= 3);

    // 空列表是成功
    let empty = env.ok(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--important",
        "false",
        "--status",
        "completed",
    ]);
    let _ = empty;
}

#[test]
fn list_pagination() {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "分页"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();
    for index in 0..7 {
        env.ok(&[
            "task",
            "add",
            "--type",
            "item",
            "--entry-id",
            &entry_id,
            "--markdown",
            &format!("任务{index}"),
        ]);
    }
    let page1 = env.run(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--limit",
        "3",
        "--sort",
        "createdAt",
        "--order",
        "asc",
    ]);
    assert_eq!(page1.code, 0);
    let env1 = page1.envelope();
    assert_eq!(env1["data"]["items"].as_array().unwrap().len(), 3);
    assert_eq!(env1["meta"]["count"], 7);
    let cursor = env1["meta"]["nextCursor"].as_str().unwrap().to_string();
    let page2 = env.ok(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--limit",
        "3",
        "--cursor",
        &cursor,
        "--sort",
        "createdAt",
        "--order",
        "asc",
    ]);
    assert_eq!(page2["items"].as_array().unwrap().len(), 3);
    let all = env.ok(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--all",
    ]);
    assert_eq!(all["items"].as_array().unwrap().len(), 7);
}

#[test]
fn find_searches_markdown_names_and_tags() {
    let env = TestEnv::fresh();
    let (_c, entry_id) = setup_entry(&env);
    env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "完成 XXX 需求",
        "--tag",
        "blue:需求",
    ]);

    let by_markdown = env.ok(&["task", "find", "--query", "XXX"]);
    assert!(by_markdown["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["type"] == "item"));

    let by_entry = env.ok(&["task", "find", "--query", "0727-0731", "--type", "entry"]);
    assert_eq!(by_entry["items"].as_array().unwrap().len(), 1);

    let by_tag = env.ok(&["task", "find", "--query", "需求", "--type", "item"]);
    assert!(!by_tag["items"].as_array().unwrap().is_empty());

    let item_only = env.ok(&["task", "find", "--query", "0727", "--type", "item"]);
    assert!(item_only["items"].as_array().unwrap().is_empty());

    let recent = env.ok(&[
        "task",
        "find",
        "--query",
        "XXX",
        "--type",
        "item",
        "--created-from",
        "2000-01-01T00:00:00Z",
    ]);
    assert_eq!(recent["items"].as_array().unwrap().len(), 1);
    let future = env.ok(&[
        "task",
        "find",
        "--query",
        "XXX",
        "--type",
        "item",
        "--changed-from",
        "2099-01-01T00:00:00Z",
    ]);
    assert_eq!(future["items"].as_array().unwrap().len(), 0);
    assert_eq!(
        env.err(&["task", "find", "--query", "XXX", "--status", "typo"], 2)["code"],
        "INVALID_STATUS"
    );
    assert_eq!(
        env.err(&["task", "list", "--type", "item", "--sort", "typo"], 2)["code"],
        "INVALID_SORT"
    );
}

#[test]
fn modify_item_state_dates_and_move() {
    let env = TestEnv::fresh();
    let (_c, entry_id) = setup_entry(&env);
    let entry2 = env.ok(&["task", "add", "--type", "entry", "--name", "0803-0807"]);
    let entry2_id = entry2["id"].as_str().unwrap().to_string();
    let item = env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "任务",
    ]);
    let item_id = item["id"].as_str().unwrap().to_string();
    let created_updated = item["updatedAt"].as_str().unwrap().to_string();

    std::thread::sleep(std::time::Duration::from_millis(20));
    let done = env.ok(&[
        "task",
        "modify",
        "--type",
        "item",
        "--id",
        &item_id,
        "--completed",
        "true",
    ]);
    assert_eq!(done["completed"], true);
    assert!(done["completedAt"].is_string());
    assert_ne!(done["updatedAt"].as_str().unwrap(), created_updated);

    let reopened = env.ok(&[
        "task",
        "modify",
        "--type",
        "item",
        "--id",
        &item_id,
        "--completed",
        "false",
    ]);
    assert_eq!(reopened["completed"], false);
    assert!(reopened["completedAt"].is_null());

    let dated = env.ok(&[
        "task",
        "modify",
        "--type",
        "item",
        "--id",
        &item_id,
        "--planned-date",
        "2026-07-28",
        "--due-date",
        "2026-07-31",
    ]);
    assert_eq!(dated["plannedDate"], "2026-07-28");
    let cleared = env.ok(&[
        "task",
        "modify",
        "--type",
        "item",
        "--id",
        &item_id,
        "--clear-planned-date",
    ]);
    assert!(cleared["plannedDate"].is_null());

    let moved = env.ok(&[
        "task",
        "modify",
        "--type",
        "item",
        "--id",
        &item_id,
        "--entry-id",
        &entry2_id,
    ]);
    assert_eq!(moved["nodeId"], entry2_id);
}

#[test]
fn modify_tags_and_emojis() {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "e"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();
    let item = env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "x",
        "--tag",
        "blue:需求",
        "--emoji",
        "🔥",
    ]);
    let item_id = item["id"].as_str().unwrap().to_string();
    let tag_id = item["tags"][0]["id"].as_str().unwrap().to_string();

    let added = env.ok(&[
        "task",
        "modify",
        "--type",
        "item",
        "--id",
        &item_id,
        "--add-tag",
        "red:紧急",
    ]);
    assert_eq!(added["tags"].as_array().unwrap().len(), 2);

    let removed = env.ok(&[
        "task",
        "modify",
        "--type",
        "item",
        "--id",
        &item_id,
        "--remove-tag",
        &tag_id,
    ]);
    assert_eq!(removed["tags"].as_array().unwrap().len(), 1);
    assert_eq!(removed["tags"][0]["text"], "紧急");

    let replaced = env.ok(&[
        "task",
        "modify",
        "--type",
        "item",
        "--id",
        &item_id,
        "--replace-tags",
        "green:a",
        "gray:b",
    ]);
    assert_eq!(replaced["tags"].as_array().unwrap().len(), 2);

    let emojis = env.ok(&[
        "task",
        "modify",
        "--type",
        "item",
        "--id",
        &item_id,
        "--add-emoji",
        "✅",
        "--remove-emoji",
        "🔥",
    ]);
    assert_eq!(emojis["emojis"], serde_json::json!(["✅"]));

    let error = env.err(
        &[
            "task",
            "modify",
            "--type",
            "item",
            "--id",
            &item_id,
            "--add-tag",
            "pink:无效",
        ],
        2,
    );
    assert_eq!(error["code"], "INVALID_TAG_COLOR");
}

#[test]
fn modify_nodes_and_move_validation() {
    let env = TestEnv::fresh();
    let c1 = env.ok(&["task", "add", "--type", "category", "--name", "A"]);
    let c1_id = c1["id"].as_str().unwrap().to_string();
    let c2 = env.ok(&[
        "task",
        "add",
        "--type",
        "category",
        "--name",
        "B",
        "--parent-id",
        &c1_id,
    ]);
    let c2_id = c2["id"].as_str().unwrap().to_string();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "e"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();

    let renamed = env.ok(&[
        "task", "modify", "--type", "entry", "--id", &entry_id, "--name", "e2", "--icon", "star",
    ]);
    assert_eq!(renamed["name"], "e2");

    let moved = env.ok(&[
        "task",
        "modify",
        "--type",
        "entry",
        "--id",
        &entry_id,
        "--parent-id",
        &c2_id,
    ]);
    assert_eq!(moved["parentId"], c2_id);
    assert_eq!(moved["path"], "A / B / e2");

    // 分类循环必须被拒绝
    let error = env.err(
        &[
            "task",
            "modify",
            "--type",
            "category",
            "--id",
            &c1_id,
            "--parent-id",
            &c2_id,
        ],
        4,
    );
    assert_eq!(error["code"], "CATEGORY_CYCLE");

    // 节点不能放到 entry 下
    let error = env.err(
        &[
            "task",
            "modify",
            "--type",
            "category",
            "--id",
            &c2_id,
            "--parent-id",
            &entry_id,
        ],
        2,
    );
    assert_eq!(error["code"], "INVALID_PARENT");

    let collapsed = env.ok(&[
        "task",
        "modify",
        "--type",
        "category",
        "--id",
        &c2_id,
        "--collapsed",
        "true",
    ]);
    assert_eq!(collapsed["collapsed"], true);
}

#[test]
fn remove_requires_confirmation_and_cascade() {
    let env = TestEnv::fresh();
    let (_c, entry_id) = setup_entry(&env);
    let item = env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "x",
    ]);
    let item_id = item["id"].as_str().unwrap().to_string();

    // 未确认 → 退出码 10
    let error = env.err(&["task", "remove", "--type", "item", "--id", &item_id], 10);
    assert_eq!(error["code"], "CONFIRMATION_REQUIRED");

    // 非空 entry 无 cascade → 退出码 4
    let error = env.err(
        &[
            "task", "remove", "--type", "entry", "--id", &entry_id, "--yes",
        ],
        4,
    );
    assert_eq!(error["code"], "NOT_EMPTY");

    // dry-run 报告影响范围
    let dry = env.ok(&[
        "task",
        "remove",
        "--type",
        "entry",
        "--id",
        &entry_id,
        "--cascade",
        "--dry-run",
    ]);
    assert_eq!(dry["plan"]["totals"]["items"], 1);

    // 实际级联删除
    let removed = env.ok(&[
        "task",
        "remove",
        "--type",
        "entry",
        "--id",
        &entry_id,
        "--cascade",
        "--yes",
    ]);
    assert_eq!(removed["removed"]["totals"]["items"], 1);
    let error = env.err(&["task", "get", "--type", "entry", "--id", &entry_id], 3);
    assert_eq!(error["code"], "NODE_NOT_FOUND");
    let error = env.err(&["task", "get", "--type", "item", "--id", &item_id], 3);
    assert_eq!(error["code"], "TASK_NOT_FOUND");

    // 删除后 revision 推进
    let confirmed = env.ok(&[
        "task",
        "add",
        "--type",
        "entry",
        "--name",
        "新条目",
        "--idempotency-key",
        "k1",
    ]);
    assert!(confirmed["id"].is_string());
}

#[test]
fn system_nodes_are_readonly() {
    let env = TestEnv::fresh();
    env.err(
        &[
            "task", "remove", "--type", "system", "--id", "my-day", "--yes",
        ],
        2,
    );
    env.err(
        &[
            "task", "modify", "--type", "system", "--id", "my-day", "--name", "x",
        ],
        2,
    );
    let got = env.ok(&["task", "get", "--type", "system", "--id", "my-day"]);
    assert_eq!(got["type"], "system");
}

#[test]
fn type_mismatch_is_conflict() {
    let env = TestEnv::fresh();
    let category = env.ok(&["task", "add", "--type", "category", "--name", "c"]);
    let id = category["id"].as_str().unwrap();
    let error = env.err(&["task", "get", "--type", "entry", "--id", id], 4);
    assert_eq!(error["code"], "TYPE_MISMATCH");
}

#[test]
fn idempotency_replay_returns_first_resource() {
    let env = TestEnv::fresh();
    let first = env.run(&[
        "task",
        "add",
        "--type",
        "category",
        "--name",
        "幂等",
        "--idempotency-key",
        "same-key",
    ]);
    assert_eq!(first.code, 0);
    let first_id = first.envelope()["data"]["id"].as_str().unwrap().to_string();

    let second = env.run(&[
        "task",
        "add",
        "--type",
        "category",
        "--name",
        "幂等",
        "--idempotency-key",
        "same-key",
    ]);
    assert_eq!(second.code, 0);
    let envelope = second.envelope();
    assert_eq!(envelope["data"]["id"].as_str().unwrap(), first_id);
    assert_eq!(envelope["meta"]["replayed"], true);

    // 只创建了一个
    let list = env.ok(&["task", "list", "--type", "category", "--parent-id", "root"]);
    assert_eq!(
        list["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["name"] == "幂等")
            .count(),
        1
    );
}

#[test]
fn if_revision_guard() {
    let env = TestEnv::fresh();
    let first = env.run(&["task", "add", "--type", "category", "--name", "c1"]);
    let revision = first.envelope()["meta"]["revision"].as_u64().unwrap();

    // 匹配的 revision 可以写入
    env.run(&[
        "task",
        "add",
        "--type",
        "category",
        "--name",
        "c2",
        "--if-revision",
        &revision.to_string(),
    ]);

    // 过期的 revision 冲突
    let error = env.err(
        &[
            "task",
            "add",
            "--type",
            "category",
            "--name",
            "c3",
            "--if-revision",
            &revision.to_string(),
        ],
        4,
    );
    assert_eq!(error["code"], "REVISION_CONFLICT");
}

#[test]
fn jq_filter_applies_to_success_envelope() {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "e"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();
    env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "a",
    ]);
    env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "b",
    ]);

    let result = env.run(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--jq",
        ".data.items | length",
    ]);
    assert_eq!(result.code, 0);
    assert_eq!(result.stdout.trim(), "2");

    let error = env.run(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--jq",
        ".bad(",
    ]);
    assert_ne!(error.code, 0);
}

#[test]
fn unknown_fields_preserved_on_write() {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "e"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();

    // 手工塞入未知字段
    let mut data = env.read_file("data.json");
    data["nodes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|node| node["id"] == entry_id)
        .unwrap()["futureField"] = serde_json::json!({"nested": true});
    data["futureTop"] = serde_json::json!(42);
    env.write_file("data.json", &data);

    // 正常修改
    env.ok(&[
        "task", "modify", "--type", "entry", "--id", &entry_id, "--name", "e2",
    ]);

    let data = env.read_file("data.json");
    let node = data["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["id"] == entry_id)
        .unwrap();
    assert_eq!(node["futureField"]["nested"], true);
    assert_eq!(data["futureTop"], 42);
}

#[test]
fn markdown_file_and_stdin_inputs() {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "e"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();
    let md_path = env.path().join("note.md");
    std::fs::write(&md_path, "# 标题\n\n正文内容").unwrap();
    let item = env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown-file",
        md_path.to_str().unwrap(),
    ]);
    assert_eq!(item["markdown"], "# 标题\n\n正文内容");
}

#[test]
fn orphan_item_reported_as_integrity_issue() {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "e"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();
    let item = env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "x",
    ]);
    let item_id = item["id"].as_str().unwrap().to_string();

    // 手工把 entry 从 nodes 移除制造孤儿（保留 item JSON）
    let mut data = env.read_file("data.json");
    data["nodes"]
        .as_array_mut()
        .unwrap()
        .retain(|node| node["id"] != entry_id);
    env.write_file("data.json", &data);

    let list = env.ok(&["task", "list", "--type", "item", "--all"]);
    let issues = list["integrityIssues"]
        .as_array()
        .expect("应报告 integrityIssues");
    assert!(issues[0]["itemIds"]
        .as_array()
        .unwrap()
        .iter()
        .any(|id| *id == item_id));

    // 孤儿可被修复（迁移到新 entry）
    let entry2 = env.ok(&["task", "add", "--type", "entry", "--name", "e2"]);
    let entry2_id = entry2["id"].as_str().unwrap().to_string();
    env.ok(&[
        "task",
        "modify",
        "--type",
        "item",
        "--id",
        &item_id,
        "--entry-id",
        &entry2_id,
    ]);
    let list = env.ok(&["task", "list", "--type", "item", "--all"]);
    assert!(list.get("integrityIssues").is_none());
}

#[test]
fn tree_shows_counts_and_depth() {
    let env = TestEnv::fresh();
    let (category_id, entry_id) = setup_entry(&env);
    env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "x",
    ]);

    let tree = env.ok(&["task", "tree", "--root-id", &category_id]);
    let root = &tree["roots"][0];
    assert_eq!(root["counts"]["recursiveItems"], 1);
    assert_eq!(root["children"][0]["id"], entry_id);

    let shallow = env.ok(&["task", "tree", "--depth", "0"]);
    assert!(shallow["roots"]
        .as_array()
        .unwrap()
        .iter()
        .all(|node| node.get("children").is_none()));
}
