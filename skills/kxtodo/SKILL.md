---
name: kxtodo
version: 1
cliHelp: kxtodo-cli --help
description: 通过 KXToDo CLI 查询、创建、修改、删除待办数据，管理定时任务与设置。
---

# KXToDo Agent SKILL

KXToDo v9 提供脚本化 CLI。本 SKILL 说明**何时调用、按什么步骤调用、如何处理风险与错误**。
字段结构、枚举、默认值一律以当前版本的 `kxtodo-cli --help` 与 `kxtodo-cli schema <目标>` 为准，本文件不复制。

## 核心约定

- 数据目录：`--data-dir` > 环境变量 `KXTODO_HOME` > 当前目录（含其 `todo-note-data` 子目录）；目录中没有数据（缺 data.json）会报错退出码 3。GUI（kxtodo.exe）数据在其同级 `todo-note-data/`。
- 输出协议：成功写 stdout，`{ "ok": true, "command", "data", "meta" }`；错误写 stderr，`{ "ok": false, "error": { "type", "code", "message", "hint?" } }`。判断成功以退出码 0 或 `ok == true` 为准。
- 退出码：0 成功；2 参数/校验错误；3 资源不存在；4 歧义或状态冲突；5 数据/锁/文件系统错误；10 高风险需确认；20 执行失败。
- `--dry-run` 永不触发确认门禁，适合先向用户展示影响范围。
- 高风险动作（删除、重置、启用/运行代码）必须 `--yes` 才执行，否则退出码 10。

## 术语

- `category`：分类文件夹，可嵌套，位于根级或另一 category 下。
- `entry`：左栏条目，位于根级或 category 下。
- `item`：条目内的具体任务（Markdown 正文 + 状态），只能归属 entry。
- `system`：我的一天/计划内/收藏等内置视图，只读。
- ID 全部不透明，**不得拼造或按前缀推断类型**；先通过 tree/list/find/get 获取。

## 选命令：list vs find

- 有真实关键词（标题、正文片段）→ `task find --query`。
- 只有范围条件（“本周”“已完成”“某条目下”）→ `task list` + 结构化过滤，**不要把范围词当关键词**。
- 不知道层级时先 `task tree` 看结构。

## 修改与删除

- 所有修改/删除按稳定 ID 进行：`task modify --type item --id ...`。
- patch 语义：字段缺省 = 不变；显式 null = 清空；数组整体替换。
- 删除一律 high-risk-write：先 `--dry-run` 看影响，再 `--yes` 执行。非空节点需 `--cascade`。

## 定时任务工作流

完整定义只走 JSON：

1. `kxtodo-cli schema schedule.spec --example interval-script --jq '.data.example' > schedule.json`
2. 编辑业务值（名称、脚本路径、时间）。
3. `kxtodo-cli schedule validate --spec @schedule.json`
4. `kxtodo-cli schedule add --spec @schedule.json --dry-run`
5. `kxtodo-cli schedule add --spec @schedule.json --yes --idempotency-key <唯一键>`

修改用 `schedule modify --id ... --patch '<json>'`；运行控制用 enable/disable/run/stop/logs/status。

## 配置与配色盘

- 点路径读取：`config get appearance.uiScale`；列表 `config list --prefix appearance`。
- 动态 map（如 `appearance.uiColors`）必须带 `--map-key <entry-id>`，键不做点路径拆解。
- `config reset` 为高风险，先 `--dry-run`。

## 幂等与并发

- 每个创建类写操作使用独立的 `--idempotency-key`，重试安全。
- 严格并发控制时读取响应 `meta.revision`，写入时携带 `--if-revision`。

## 典型流程

**“本周做了什么”**：
1. `task tree` 或 `task find --type entry --query "0727-0731"` 定位条目。
2. `task list --type item --changed-from <周一> --changed-to <周日> --all` 取本周创建/修改。
3. `task list --type item --entry-id <id> --all` 取条目全量，再自行汇总。

**创建本周任务**：
1. `task list --type category --parent-id root` / `task find` 查找目标分类与条目。
2. 不存在则依次 `task add --type category`、`task add --type entry`（记录返回 ID）。
3. `task add --type item --entry-id <id> --markdown "..." --idempotency-key <键>`。

## 错误处理

- 退出码 3：资源不存在 → 先 find 获取真实 ID。
- 退出码 4：类型不符/状态冲突 → 检查 --type 与对象真实类型；非空删除需 --cascade。
- 退出码 10：高风险未确认 → 向用户说明影响后加 --yes。
- 退出码 2：参数问题 → 看 error.hint，并对照 `kxtodo-cli <领域> <动作> --help`。
