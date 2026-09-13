#![recursion_limit = "512"]
//! storage 域回归（v0.7.3）：usage 的统计口径 + clean 的确认门与删除范围。
//!
//! 铁律：领域 JSON、runtime/、backups/（只统计）、被引用的图片、最新两份与今天的
//! 服务器日志都必须活过 clean；freedBytes 恰好等于消失文件的字节和。

mod common;

use std::path::Path;

use chrono::{Duration as ChronoDuration, Local};
use common::TestEnv;
use kxtodo_core::core::{execute, Controls, ExecContext, Invocation};
use kxtodo_core::repo::Repository;
use serde_json::{json, Value};

fn write_sized(path: &Path, len: usize) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, vec![b'k'; len]).unwrap();
}

/// gui.* 没有 CLI 面：直接过核心执行器（与 image_gc 测试同一套路）。
fn dispatch(env: &TestEnv, command: &str, params: Value) -> Value {
    let repo = Repository::open(env.path()).unwrap();
    let invocation = Invocation {
        command: command.to_string(),
        params,
        controls: Controls {
            yes: true,
            ..Default::default()
        },
    };
    let ctx = ExecContext {
        repo: &repo,
        cwd: env.path(),
        host: None,
        custom_data_dir: true,
    };
    let outcome = execute(&invocation, &ctx);
    assert_eq!(outcome.code, 0, "envelope: {}", outcome.envelope);
    outcome.envelope["data"].clone()
}

struct Fixture {
    env: TestEnv,
    node_id: String,
    logs: Vec<String>, // [今天, 前1天, 前2天, 前3天, 前4天] 的文件名
}

/// 建好一个「什么都有一点」的数据目录。所有领域写操作都在图片落盘**之前**完成——
/// 保存即清理（v0.7.3 没有宽限窗）会把先落盘的孤儿图当场扫掉，测试要造的就是
/// 「已经躺在盘上、等 clean 来收」的孤儿。
fn fixture() -> Fixture {
    let env = TestEnv::fresh();
    let entry = env.ok(&["task", "add", "--type", "entry", "--name", "带图条目"]);
    let node_id = entry["id"].as_str().unwrap().to_string();
    env.ok(&[
        "task", "add", "--type", "item", "--entry-id", &node_id,
        "--markdown", "看图 ![](ref.png)",
    ]);
    env.ok(&[
        "task", "add", "--type", "item", "--entry-id", &node_id,
        "--markdown", "没图的卡片",
    ]);
    env.ok(&["diary", "add", "--date", "2026-09-01", "--markdown", "日记 ![](dref.png)"]);
    // 背景与头像的引用关系
    dispatch(
        &env,
        "gui.set-background",
        json!({ "nodeId": node_id, "image": "img:bg-used.png" }),
    );
    env.ok(&["config", "set", "profile.avatar", "ava-used.png"]);
    env.ok(&["config", "set", "diary.backgroundImage", "img:bg-diary.png"]);

    let root = env.path();
    // 插图：引用的 + 孤儿的 + 整目录孤儿（id 不是现存节点）+ 日记目录两种
    let entry_dir = root.join("img").join("data").join(&node_id);
    write_sized(&entry_dir.join("ref.png"), 10);
    write_sized(&entry_dir.join("orphan.png"), 11);
    write_sized(&root.join("img").join("data").join("missing-id").join("whatever.png"), 12);
    let diary_dir = root.join("img").join("data").join("diary");
    write_sized(&diary_dir.join("dref.png"), 13);
    write_sized(&diary_dir.join("dorphan.png"), 14);
    // 背景与头像
    let bg_dir = root.join("img").join("background");
    write_sized(&bg_dir.join("bg-used.png"), 30);
    write_sized(&bg_dir.join("bg-orphan.png"), 31);
    write_sized(&bg_dir.join("bg-diary.png"), 32);
    write_sized(&bg_dir.join("bg-x.png.part"), 21); // 原子写残留，clean 顺手收走
    let ava_dir = root.join("img").join("avator");
    write_sized(&ava_dir.join("ava-used.png"), 34);
    write_sized(&ava_dir.join("ava-orphan.png"), 33);
    // 崩溃残留的临时文件（数据目录根，.*.tmp）
    write_sized(&root.join(".data.json.999.tmp"), 40);
    // 服务器日志：今天 + 前四天（保留最新两份且不删今天 → 前三天的三份可清理）
    let mut logs = Vec::new();
    for days_ago in 0..5 {
        let day = (Local::now() - ChronoDuration::days(days_ago))
            .format("%Y%m%d")
            .to_string();
        let name = format!("server-{day}.log");
        write_sized(&root.join("server").join("log").join(&name), 50 + days_ago as usize);
        logs.push(name);
    }
    // 必须活过 clean 的东西
    write_sized(&root.join("runtime").join("sync.json"), 60);
    write_sized(&root.join("backups").join("2026-01-01-manual").join("data.json"), 70);

    Fixture { env, node_id, logs }
}

#[test]
fn usage_reports_exact_counts_and_bytes() {
    let fx = fixture();
    let usage = fx.env.ok(&["storage", "usage"]);

    assert_eq!(
        usage["dataDir"].as_str().unwrap(),
        fx.env.path().display().to_string()
    );
    assert!(usage["totalBytes"].as_u64().unwrap() > 0);

    assert_eq!(usage["images"], json!({ "count": 5, "bytes": 60 }));
    assert_eq!(usage["orphanImages"], json!({ "count": 3, "bytes": 37 }));
    // 背景目录里的 .part 也计入体积，但不算「孤儿背景」（它归临时残留清理）
    assert_eq!(usage["backgrounds"], json!({ "count": 4, "bytes": 114 }));
    assert_eq!(usage["orphanBackgrounds"], json!({ "count": 1, "bytes": 31 }));
    assert_eq!(usage["avatars"], json!({ "count": 2, "bytes": 67 }));
    assert_eq!(usage["orphanAvatars"], json!({ "count": 1, "bytes": 33 }));
    assert_eq!(usage["tempFiles"], json!({ "count": 1, "bytes": 40 }));
    assert_eq!(usage["serverLogs"], json!({ "count": 5, "bytes": 260 }));
    assert_eq!(usage["cleanableLogs"], json!({ "count": 3, "bytes": 159 }));
    assert_eq!(usage["backups"], json!({ "count": 1, "bytes": 70 }));
}

#[test]
fn clean_is_gated_and_deletes_exactly_the_orphans() {
    let fx = fixture();
    let root = fx.env.path();

    // 未带 --yes：确认门（退出码 10），分毫不动
    let refused = fx.env.err(&["storage", "clean"], 10);
    assert_eq!(refused["code"], "CONFIRMATION_REQUIRED");
    assert_eq!(refused["type"], "confirmation_required");
    assert!(root.join("img").join("data").join(&fx.node_id).join("orphan.png").exists());

    // dry-run：给出计划，不动文件
    let dry = fx.env.ok(&["storage", "clean", "--dry-run"]);
    assert_eq!(dry["dryRun"], true);
    assert_eq!(dry["plan"]["orphanImages"], 3);
    assert_eq!(dry["plan"]["estimatedFreedBytes"], 321);
    assert!(root.join("img").join("data").join(&fx.node_id).join("orphan.png").exists());

    let result = fx.env.ok(&["storage", "clean", "--yes"]);
    assert_eq!(result["removedImages"], 3);
    assert_eq!(result["removedBackgrounds"], 1);
    assert_eq!(result["removedAvatars"], 1);
    assert_eq!(result["removedTempFiles"], 2, ".tmp 与 img/ 下的 .part 一起计");
    assert_eq!(result["removedLogs"], 3);
    assert_eq!(result["warnings"], json!([]));
    // freedBytes = 37(孤儿插图) + 31(孤儿背景) + 33(孤儿头像) + 40(.tmp) + 21(.part) + 159(过期日志)
    assert_eq!(result["freedBytes"], 321);

    // 被引用的图片全部活着
    let entry_dir = root.join("img").join("data").join(&fx.node_id);
    assert!(entry_dir.join("ref.png").exists());
    assert!(!entry_dir.join("orphan.png").exists());
    assert!(root.join("img").join("data").join("diary").join("dref.png").exists());
    assert!(!root.join("img").join("data").join("diary").join("dorphan.png").exists());
    // 整目录孤儿连目录一起收走（id 不是现存节点）
    assert!(!root.join("img").join("data").join("missing-id").exists());
    // 现存节点与 diary 的目录本身保留
    assert!(entry_dir.is_dir());
    assert!(root.join("img").join("data").join("diary").is_dir());

    assert!(root.join("img").join("background").join("bg-used.png").exists());
    assert!(root.join("img").join("background").join("bg-diary.png").exists());
    assert!(!root.join("img").join("background").join("bg-orphan.png").exists());
    assert!(!root.join("img").join("background").join("bg-x.png.part").exists());
    assert!(root.join("img").join("avator").join("ava-used.png").exists());
    assert!(!root.join("img").join("avator").join("ava-orphan.png").exists());
    assert!(!root.join(".data.json.999.tmp").exists());

    // 日志：最新两份（今天 + 前 1 天）保留，其余删掉
    let log_dir = root.join("server").join("log");
    assert!(log_dir.join(&fx.logs[0]).exists(), "今天的日志绝不删");
    assert!(log_dir.join(&fx.logs[1]).exists(), "最新两份保留");
    assert!(!log_dir.join(&fx.logs[2]).exists());
    assert!(!log_dir.join(&fx.logs[3]).exists());
    assert!(!log_dir.join(&fx.logs[4]).exists());

    // 领域文件、runtime、backups 一根汗毛都不能少
    for name in ["data.json", "settings.json", "diary.json"] {
        assert!(root.join(name).is_file(), "{name} 必须活着");
    }
    assert_eq!(
        std::fs::read(root.join("runtime").join("sync.json")).unwrap().len(),
        60
    );
    assert_eq!(
        std::fs::read(root.join("backups").join("2026-01-01-manual").join("data.json"))
            .unwrap()
            .len(),
        70
    );

    // 再清一次：无可清，零结果且无 warning
    let again = fx.env.ok(&["storage", "clean", "--yes"]);
    assert_eq!(again["freedBytes"], 0);
    assert_eq!(again["removedImages"], 0);
    assert_eq!(again["removedBackgrounds"], 0);
    assert_eq!(again["removedAvatars"], 0);
    assert_eq!(again["removedTempFiles"], 0);
    // 今天的与最新那份日志永远在保留集里
    assert_eq!(again["removedLogs"], 0);
    assert_eq!(again["warnings"], json!([]));
}
