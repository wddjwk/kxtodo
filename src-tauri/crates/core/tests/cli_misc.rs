#![recursion_limit = "512"]
//! version / schema / skills / doctor / jq / help / formats (§3.1, §3.2, §3.7).

mod common;

use common::TestEnv;
use serde_json::Value;
use kxtodo_core::jq;

#[test]
fn version_reports_schema_versions() {
    let env = TestEnv::fresh();
    let data = env.ok(&["version"]);
    assert_eq!(data["schemaVersions"]["data"], 6);
    assert_eq!(data["schemaVersions"]["settings"], 1);
    assert_eq!(data["schemaVersions"]["schedule"], 2);
}

#[test]
fn help_exits_zero_at_every_level() {
    let env = TestEnv::fresh();
    for args in [
        vec!["--help"],
        vec!["task", "--help"],
        vec!["task", "add", "--help"],
        vec!["schedule", "--help"],
        vec!["config", "set", "--help"],
    ] {
        let result = env.run(&args);
        assert_eq!(result.code, 0, "{args:?} 帮助应退出 0：{}", result.stderr);
        assert!(
            result.stderr.contains("Risk")
                || result.stderr.contains("用法")
                || result.stderr.contains("Usage")
                || result.stderr.contains("KXToDo")
        );
    }
}

#[test]
fn action_help_shows_risk_level() {
    let env = TestEnv::fresh();
    let result = env.run(&["task", "remove", "--help"]);
    assert!(result.stderr.contains("high-risk-write"));
    let result = env.run(&["task", "add", "--help"]);
    assert!(result.stderr.contains("Risk: write"));
    let result = env.run(&["task", "get", "--help"]);
    assert!(result.stderr.contains("Risk: read"));
}

#[test]
fn unknown_command_is_exit_2() {
    let env = TestEnv::fresh();
    let result = env.run(&["frobnicate"]);
    assert_eq!(result.code, 2);
    let envelope = result.stderr_envelope();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "CLI_ARGUMENT_ERROR");
    assert!(envelope["meta"]["requestId"].is_string());
}

#[test]
fn invalid_format_and_jq_syntax_do_not_write() {
    let env = TestEnv::fresh();
    let bad_format = env.run(&[
        "task", "add", "--type", "category", "--name", "bad", "--format", "xml",
    ]);
    assert_eq!(bad_format.code, 2);
    assert!(!env.file_exists("data.json"));

    let bad_jq = env.run(&[
        "task", "add", "--type", "category", "--name", "bad", "--jq", ".data(",
    ]);
    assert_eq!(bad_jq.code, 2);
    assert!(!env.file_exists("data.json"));
}

#[test]
fn schema_spec_and_patch_come_from_model() {
    let env = TestEnv::fresh();
    let spec = env.ok(&["schema", "schedule.spec"]);
    assert!(spec.get("properties").is_some() || spec.get("$ref").is_some());
    let text = spec.to_string();
    assert!(text.contains("missedPolicy"));
    assert!(text.contains("stopWhen"));

    let patch = env.ok(&["schema", "schedule.patch"]);
    // patch schema 递归移除了 required
    fn has_required(value: &Value) -> bool {
        match value {
            Value::Object(map) => map.contains_key("required") || map.values().any(has_required),
            Value::Array(items) => items.iter().any(has_required),
            _ => false,
        }
    }
    assert!(!has_required(&patch), "patch schema 不得包含 required");

    let notification = env.ok(&["schema", "notification"]);
    assert!(notification.to_string().contains("duration"));
    let matcher = env.ok(&["schema", "match"]);
    assert!(matcher.to_string().contains("pattern"));
}

#[test]
fn schema_command_introspection() {
    let env = TestEnv::fresh();
    let add = env.ok(&["schema", "task.add"]);
    assert_eq!(add["command"], "task.add");
    assert_eq!(add["risk"], "write");
    assert!(add["params"]["--type"].is_object());
    let remove = env.ok(&["schema", "task.remove"]);
    assert_eq!(remove["risk"], "high-risk-write");
    env.err(&["schema", "task.nope"], 3);
}

#[test]
fn schema_jq_lists_supported_subset() {
    let env = TestEnv::fresh();
    let doc = env.ok(&["schema", "jq"]);
    assert!(doc["syntax"].is_array());
    assert!(doc["examples"].is_array());
}

#[test]
fn jq_subset_behaviour() {
    let input = serde_json::json!({
        "data": {
            "items": [
                { "id": "a", "completed": false, "markdown": "任务A" },
                { "id": "b", "completed": true, "markdown": "任务B" },
            ],
            "count": 2,
        }
    });
    assert_eq!(jq::apply(".data.count", &input).unwrap(), 2);
    assert_eq!(
        jq::apply(".data.items | length", &input).unwrap(),
        serde_json::json!(2)
    );
    assert_eq!(
        jq::apply(".data.items[0].id", &input).unwrap(),
        serde_json::json!("a")
    );
    assert_eq!(
        jq::apply(".data.items[-1].id", &input).unwrap(),
        serde_json::json!("b")
    );
    assert_eq!(
        jq::apply(".data.items[] | .id", &input).unwrap(),
        serde_json::json!(["a", "b"])
    );
    assert_eq!(
        jq::apply(".data.items | map(.id)", &input).unwrap(),
        serde_json::json!(["a", "b"])
    );
    assert_eq!(
        jq::apply(".data.items[] | select(.completed == false) | .id", &input).unwrap(),
        serde_json::json!("a")
    );
    assert_eq!(
        jq::apply(".data.items | keys", &input).unwrap_err().code,
        "JQ_TYPE"
    );
    assert!(jq::apply(".data | length", &input).is_ok());
    assert!(jq::apply("..", &input).is_err(), "递归下降不在子集内");
    assert!(jq::apply(".data.items | sort_by(.id)", &input).is_err());
}

#[test]
fn skills_commands_work_embedded() {
    let env = TestEnv::fresh();
    let list = env.ok(&["skills", "list"]);
    assert_eq!(list["skills"][0]["name"], "kxtodo");
    assert_eq!(list["skills"][0]["available"], true);
    assert_eq!(list["skills"][0]["source"], "embedded");

    let read = env.ok(&["skills", "read", "kxtodo"]);
    let content = read["content"].as_str().unwrap();
    // 断言结构而不是某一行的原文：文档会重写（v0.8.0 就重排过一次），
    // 但 frontmatter 与 H1 必须在，`skills validate` 也靠 frontmatter 判 name/version。
    assert!(content.contains("name: kxtodo"), "SKILL 必须有 frontmatter name");
    assert!(content.contains("# KXToDo"), "SKILL 必须有一个 H1 标题");
    assert_eq!(
        read["version"].as_u64(),
        Some(kxtodo_core::skills::SKILL_VERSION as u64),
        "frontmatter 的 version 必须与 SKILL_VERSION 同步抬"
    );
    assert_eq!(read["source"], "embedded");

    env.err(&["skills", "read", "unknown"], 3);

    let path = env.ok(&["skills", "path"]);
    assert_eq!(path["source"], "embedded");

    let validate = env.ok(&["skills", "validate"]);
    assert!(
        validate["valid"].as_bool().unwrap(),
        "SKILL 校验应通过：{validate}"
    );

    let persisted = env.ok(&["skills", "persist", "kxtodo", env.path().to_str().unwrap()]);
    let expected = env.path().join("skills").join("kxtodo").join("SKILL.md");
    assert_eq!(persisted["path"], expected.display().to_string());
    assert_eq!(
        std::fs::read_to_string(&expected).unwrap(),
        read["content"].as_str().unwrap()
    );
    let persisted_again = env.ok(&[
        "skills",
        "persist",
        "kxtodo",
        env.path().join("skills").to_str().unwrap(),
    ]);
    assert_eq!(persisted_again["path"], expected.display().to_string());

    let blocked_root = env.path().join("blocked-export");
    let blocked = env.run(&[
        "skills",
        "persist",
        "kxtodo",
        blocked_root.to_str().unwrap(),
        "--idempotency-key",
        "unsupported",
    ]);
    assert_eq!(blocked.code, 2);
    assert!(!blocked_root.exists());

    let echo = env.run(&["skills", "echo", "kxtodo"]);
    assert_eq!(echo.code, 0);
    assert_eq!(echo.stdout, read["content"].as_str().unwrap());
    assert!(serde_json::from_str::<Value>(&echo.stdout).is_err());
}

#[test]
fn doctor_on_fresh_dir() {
    let env = TestEnv::fresh();
    let report = env.ok(&["doctor"]);
    assert!(report["checks"].as_array().unwrap().len() >= 8);
    assert!(
        report["healthy"].as_bool().unwrap(),
        "新目录应健康：{report}"
    );
}

#[test]
fn doctor_check_is_selective_and_read_only() {
    let env = TestEnv::fresh();
    env.ok(&["task", "tree"]);
    for path in [
        env.path().join("img").join("avator"),
        env.path().join("img").join("background"),
        env.path().join("img").join("data"),
    ] {
        let _ = std::fs::remove_dir_all(path);
    }
    env.ok(&["doctor"]);
    assert!(!env.path().join("img").join("avator").exists());
    assert!(!env.path().join("img").join("background").exists());
    assert!(!env.path().join("img").join("data").exists());

    let temp = env.path().join(".data.json.crash.tmp");
    std::fs::write(&temp, "sentinel").unwrap();
    let report = env.ok(&["doctor", "--check", "runtimes"]);
    let checks = report["checks"].as_array().unwrap();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0]["name"], "runtimes");
    assert_eq!(std::fs::read_to_string(&temp).unwrap(), "sentinel");
    let error = env.err(&["doctor", "--check", "nope"], 2);
    assert_eq!(error["code"], "UNKNOWN_DOCTOR_CHECK");
}

#[test]
fn doctor_reports_corrupted_data() {
    let env = TestEnv::fresh();
    env.ok(&["task", "tree"]);
    std::fs::write(env.path().join("data.json"), "{ bad").unwrap();
    let report = env.ok(&["doctor"]);
    assert_eq!(report["healthy"], false);
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["name"] == "data" && check["ok"] == false));
}

#[test]
fn output_formats_table_pretty_ndjson() {
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
        "任务甲",
    ]);
    env.ok(&[
        "task",
        "add",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--markdown",
        "任务乙",
    ]);

    let table = env.run(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--format",
        "table",
    ]);
    assert_eq!(table.code, 0);
    assert!(table.stdout.contains("MARKDOWN"));
    assert!(table.stdout.contains("任务甲"));

    let pretty = env.run(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--format",
        "pretty",
    ]);
    assert_eq!(pretty.code, 0);
    assert!(pretty.stdout.contains("[ ] 任务甲"));

    let ndjson = env.run(&[
        "task",
        "list",
        "--type",
        "item",
        "--entry-id",
        &entry_id,
        "--format",
        "ndjson",
    ]);
    assert_eq!(ndjson.code, 0);
    let lines: Vec<&str> = ndjson.stdout.lines().collect();
    assert_eq!(lines.len(), 2);
    for line in lines {
        let parsed: Value = serde_json::from_str(line).unwrap();
        assert_eq!(parsed["type"], "item");
    }

    // 错误信封不受格式影响
    let error = env.run(&[
        "task", "get", "--type", "item", "--id", "nope", "--format", "table",
    ]);
    assert_eq!(error.code, 3);
    let parsed: Value = serde_json::from_str(&error.stderr).unwrap();
    assert_eq!(parsed["ok"], false);
}

#[test]
fn persisted_read_commands_return_revision_meta() {
    let env = TestEnv::fresh();
    env.ok(&["task", "tree"]);
    for args in [
        vec!["task", "tree"],
        vec!["task", "list", "--type", "entry"],
        vec!["schedule", "list"],
        vec!["schedule", "status"],
        vec!["schedule", "runtime", "list"],
        vec!["config", "list"],
        vec!["config", "path"],
        vec!["config", "validate"],
    ] {
        let result = env.run(&args);
        assert_eq!(result.code, 0, "{args:?}: {}", result.stderr);
        let envelope = result.envelope();
        assert!(envelope["meta"]["revisionDomain"].is_string(), "{args:?}");
        assert!(envelope["meta"]["revision"].is_number(), "{args:?}");
    }
}

#[test]
fn duration_parser_is_shared() {
    use kxtodo_core::time::{format_duration, parse_duration_ms};
    assert_eq!(parse_duration_ms("500ms").unwrap(), 500);
    assert_eq!(parse_duration_ms("5s").unwrap(), 5000);
    assert_eq!(parse_duration_ms("10m").unwrap(), 600_000);
    assert_eq!(parse_duration_ms("1h").unwrap(), 3_600_000);
    assert_eq!(parse_duration_ms("2d").unwrap(), 172_800_000);
    assert!(parse_duration_ms("0s").is_err());
    assert!(parse_duration_ms("5x").is_err());
    assert!(parse_duration_ms("").is_err());
    assert_eq!(format_duration(86_400_000), "1d");
    assert_eq!(format_duration(3_600_000), "1h");
    assert_eq!(format_duration(90_000), "90s");
    assert_eq!(format_duration(5200), "5200ms");
}

#[test]
fn schema_exposes_limit_defaults_only_where_they_exist() {
    let env = TestEnv::fresh();
    // task / schedule 家族的 --limit 真有默认值：clap 的 default_value 让 schema 能机读到
    for path in ["task.list", "task.find", "schedule.list", "schedule.find"] {
        let schema = env.ok(&["schema", path]);
        assert_eq!(
            schema["params"]["--limit"]["default"],
            serde_json::json!(["50"]),
            "{path} 的 --limit 应带机读默认值"
        );
        assert!(
            schema["params"]["--limit"]["description"]
                .as_str()
                .unwrap()
                .contains("50"),
            "{path} 的帮助文案也要写出默认值"
        );
    }
    let logs = env.ok(&["schema", "schedule.logs"]);
    assert_eq!(logs["params"]["--limit"]["default"], serde_json::json!(["20"]));

    // ledger / diary 的 --limit **刻意没有**默认值：不传就返回全部（记账是金融数据，
    // 静默截断会让月合计算出残值）；但它们同样支持 --cursor / --all。
    for path in ["ledger.list", "diary.list"] {
        let schema = env.ok(&["schema", path]);
        assert!(
            schema["params"]["--limit"].get("default").is_none(),
            "{path} 不该有默认 limit，否则等于改了默认行为"
        );
        assert!(
            schema["params"]["--limit"]["description"]
                .as_str()
                .unwrap()
                .contains("不传则返回全部"),
            "{path} 的帮助文案要说清楚不传就是全部"
        );
        assert!(schema["params"]["--cursor"].is_object(), "{path} 要有 --cursor");
        assert!(schema["params"]["--all"].is_object(), "{path} 要有 --all");
    }
}

#[test]
fn jq_object_construction_and_comma_output() {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "e"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();
    env.ok(&["task", "add", "--type", "item", "--entry-id", &entry_id, "--markdown", "任务甲"]);
    env.ok(&["task", "add", "--type", "item", "--entry-id", &entry_id, "--markdown", "任务乙"]);

    // {a, b}：简写取同名字段，`键: 路径` 是显式键名——常配 map() 把列表削成需要的字段
    let objects = env.run(&[
        "task", "list", "--type", "item", "--entry-id", &entry_id,
        "--jq", ".data.items | map({markdown, done: .completed})",
    ]);
    assert_eq!(objects.code, 0, "{}", objects.stderr);
    assert_eq!(
        serde_json::from_str::<Value>(&objects.stdout).unwrap(),
        serde_json::json!([
            { "markdown": "任务甲", "done": false },
            { "markdown": "任务乙", "done": false }
        ])
    );

    // 顶层逗号：多个值按既有约定序列化成数组
    let pair = env.run(&["task", "list", "--type", "item", "--jq", ".command, .ok"]);
    assert_eq!(pair.code, 0, "{}", pair.stderr);
    assert_eq!(
        serde_json::from_str::<Value>(&pair.stdout).unwrap(),
        serde_json::json!(["task.list", true])
    );

    // `,` 的优先级高于 `|`：`.a | .b, .c` = `.a | (.b, .c)`（两个分支都吃 items[0]，
    // 不是「(.a | .b), .c」——那样第二个分支会去信封根上找 .completed 得到 null）
    let nested = env.run(&[
        "task", "list", "--type", "item", "--entry-id", &entry_id,
        "--jq", ".data.items[0] | .markdown, .completed",
    ]);
    assert_eq!(nested.code, 0, "{}", nested.stderr);
    assert_eq!(
        serde_json::from_str::<Value>(&nested.stdout).unwrap(),
        serde_json::json!(["任务甲", false])
    );

    // 写错就是退出码 2，且 hint 里内联了支持清单（不必再跑一次 schema jq）
    let bad = env.run(&["task", "tree", "--jq", "{,id}"]);
    assert_eq!(bad.code, 2);
    let hint = bad.stderr_envelope()["error"]["hint"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    for token in ["{a, b}", ".a, .b", "map(", "select(", "schema jq"] {
        assert!(hint.contains(token), "hint 里应内联 `{token}`：{hint}");
    }
}

#[test]
fn jq_overrides_format_but_says_so_on_stderr_only() {
    let env = TestEnv::fresh();
    env.ok(&["task", "add", "--type", "category", "--name", "c"]);

    let result = env.run(&[
        "task", "list", "--type", "category", "--format", "table", "--jq", ".data.items | length",
    ]);
    assert_eq!(result.code, 0, "{}", result.stderr);
    // stdout 必须仍是可被脚本解析的纯 JSON——提示一个字都不能进去
    assert_eq!(result.stdout.trim(), "1");
    assert!(
        serde_json::from_str::<Value>(result.stdout.trim()).is_ok(),
        "stdout 不是纯 JSON：{}",
        result.stdout
    );
    assert!(
        result.stderr.contains("--jq") && result.stderr.contains("table"),
        "stderr 应说明 --format table 被忽略：{}",
        result.stderr
    );

    // 不冲突时（缺省 = json）不给提示
    let quiet = env.run(&["task", "list", "--type", "category", "--jq", ".data.items | length"]);
    assert_eq!(quiet.code, 0, "{}", quiet.stderr);
    assert_eq!(quiet.stderr, "", "默认 json 不该有提示：{}", quiet.stderr);

    // --jq 自己失败时 stderr 仍然只是错误信封（调用方要解析它，不能拼提示）
    let failed = env.run(&[
        "task", "list", "--type", "category", "--format", "pretty", "--jq", ".data.items | keys",
    ]);
    assert_eq!(failed.code, 2);
    let envelope = failed.stderr_envelope();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["error"]["code"], "JQ_TYPE");
}

/// 文档里的每一段 `示例：kxtodo-cli ...` 都必须真的能解析。
///
/// 早先 `ledger account-remove` 那三条写成了 core 内部名（`accountRemove` /
/// `categoryModify` / `categoryRemove`），照抄必失败——而 help 是用户与 Agent
/// 唯一会照着敲的东西。这条测试把「示例」纳入回归：命令树一改名，示例不改就红。
#[test]
fn help_examples_parse() {
    use clap::{CommandFactory, Parser};
    use kxtodo_core::cli::Cli;

    /// 收集所有层级 `long_about` 里的示例命令行
    fn collect(cmd: &clap::Command, out: &mut Vec<String>) {
        for sub in cmd.get_subcommands() {
            let path = sub.get_name().to_string();
            if let Some(text) = sub.get_long_about().or_else(|| sub.get_about()) {
                for line in text.to_string().lines() {
                    let line = line.trim();
                    let Some(rest) = line.strip_prefix("kxtodo-cli ") else {
                        continue;
                    };
                    // 跳过元语法（`<command>`、`a|b` 这类占位写法）
                    if rest.contains('<') || rest.contains('|') || rest.contains('…') {
                        continue;
                    }
                    out.push(rest.to_string());
                }
            }
            let _ = path;
            collect(sub, out);
        }
    }

    /// 按 shell 的习惯切词（示例里会写 `--markdown "完成 XXX 需求"`），
    /// 直接 split_whitespace 会把引号里的内容拆成好几个参数。
    fn split_shell(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = String::new();
        let mut quote: Option<char> = None;
        for ch in line.chars() {
            match quote {
                Some(mark) if ch == mark => quote = None,
                Some(_) => current.push(ch),
                None if ch == '"' || ch == '\'' => quote = Some(ch),
                // `#` 起头的词是行尾注释（示例里会写「# 这个月写了几篇」这类说明）
                None if ch == '#' && current.is_empty() => break,
                None if ch.is_whitespace() => {
                    if !current.is_empty() {
                        out.push(std::mem::take(&mut current));
                    }
                }
                None => current.push(ch),
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
        out
    }

    let root = Cli::command();
    let mut examples = Vec::new();
    collect(&root, &mut examples);
    assert!(
        examples.len() >= 20,
        "示例数量太少（{}），收集逻辑可能已经失效",
        examples.len()
    );
    for example in examples {
        let args = split_shell(&example);
        let parsed = Cli::try_parse_from(std::iter::once("kxtodo-cli".to_string()).chain(args));
        if let Err(error) = parsed {
            use clap::error::ErrorKind;
            // 只关心「命令/参数名写错」这两类：示例里的占位值（id、路径）不参与解析，
            // 但缺少必需值这类错误在示例里是正常的（示例常常只演示一部分参数）
            assert!(
                !matches!(
                    error.kind(),
                    ErrorKind::InvalidSubcommand | ErrorKind::UnknownArgument
                ),
                "help 示例解析不过：`kxtodo-cli {example}`\n{error}"
            );
        }
    }
}

/// v0.8.1：`-` 开头的文本值必须能传进去（markdown 列表、破折号开头的备注都是常见写法）。
#[test]
fn hyphen_leading_text_values_are_accepted() {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "收件箱"]);
    let entry_id = entry["id"].as_str().unwrap().to_string();

    let item = env.ok(&[
        "task", "add", "--type", "item", "--entry-id", &entry_id,
        "--markdown", "- 列表项一\n- 列表项二",
    ]);
    assert_eq!(item["markdown"], "- 列表项一\n- 列表项二");
    // 破折号开头的备注（记账的 --note 是自由文本）
    let account = env.ok(&["ledger", "account-add", "--name", "零钱", "--yes"]);
    assert!(account["id"].as_str().is_some());
    let entry_note = env.ok(&[
        "ledger", "add", "--amount", "12.50", "--account", "零钱", "--note", "-备注", "--yes",
    ]);
    assert_eq!(entry_note["note"], "-备注");

    let diary = env.ok(&[
        "diary", "add", "--markdown", "- 今天做完了这些\n- 还有这些",
    ]);
    assert!(diary["id"].as_str().unwrap().starts_with("diary-"));

    // 期初为负（信用卡）：`--initial -1234.56` 不加等号也要认
    let account = env.ok(&[
        "ledger", "account-add", "--name", "信用卡", "--initial", "-1234.56", "--yes",
    ]);
    assert_eq!(account["initialCents"], -123456);

    // 但金额本身不许为负：报业务错误（不是 clap 的「未知参数」）
    let bad = env.run(&[
        "ledger", "add", "--amount", "-30", "--account", "信用卡", "--yes",
    ]);
    assert_eq!(bad.code, 2, "{}", bad.stderr);
    assert_eq!(bad.stderr_envelope()["error"]["code"], "LEDGER_AMOUNT_INVALID");
}
