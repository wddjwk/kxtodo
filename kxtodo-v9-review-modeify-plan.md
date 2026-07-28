# KXTodo v9.0.0 CLI 改动计划书

- **来源**：`docs/kxtodo-v9-cli-code-review.md`（含第二轮核实）、`docs/kxtodo-v9-cli-requirements.md`
- **目标**：修复发布阻断问题，使 v9 CLI 达到需求定义的数据安全、调度可靠性与协议契约。
- **用法**：按批次顺序落实；每项给出"改哪里 / 为什么 / 改动方向 / 验收"。工程师结合代码自行实现。
- **规模**：1 个架构级改造（调度器）+ 约 15 个局部修复 + 测试。粗估 9–15 人日。
- **规模标记**：🔴大 🟡中 🟢小。

---

## 批次一：数据安全 + 调度可靠性（发布阻断，最高优先）

### 1.1 🟡 桌面 GUI 禁止 legacy 全量写降级（§3.5）
- **改哪里**：`src/lib/backend.ts`(`hasCoreDispatch`)、`src/lib/stores.ts`(`hydrate`)、`src-tauri/src/lib.rs`(`save_state/save_settings/save_scheduler`)。
- **为什么**：`hasCoreDispatch` 把 `core_snapshot` 的任意异常（含数据损坏 `DATA_CORRUPTED`）都当成"Core 不可用"→回退 legacy 全量写→**直接覆盖损坏的原文件**，违反"损坏数据不得被默认空数据覆盖"。
- **改动方向**：capability 探测改用不读业务数据的专用 ping（区分"Core 不可用"与"数据损坏"）；桌面端 Core 初始化失败时显示恢复错误并**禁止写入**；`save_*` 全量写命令仅在 mobile/browser 的 cfg 分支注册，桌面不注册或明确拒绝。
- **验收**：`data.json` 损坏时，GUI 修改一次不改写原文件；仅 mobile/browser 保留 legacy。

### 1.2 🔴 调度器改为异步执行模型（§3.1 + §4.2 + §4.4 + 补充 B）
- **改哪里**：`src-tauri/src/domain/scheduler.rs`、`exec.rs`(`ProcessRegistry`)、`host.rs`(`shutdown_host`/`run_schedule_now`/`stop_schedule`)。
- **为什么**：当前单线程串行处理 `tick/RunNow/Stop/Shutdown`，`run_now(false)` 同步阻塞到进程结束才返回；长任务运行期间 `stop`/`shutdown`/其他到期任务全部被阻塞；崩溃后 `running=true` 永久卡死；shutdown 不回收子进程。且一次运行发起 3~4 次写事务（写放大）。
- **改动方向**：
  - 控制线程只做计划/状态机/消息路由；每个 schedule 的执行放入独立 worker，按 ID 注册运行槽（同 ID 不重入、不同 ID 可并发）。
  - `RunNow{wait:false}` 成功创建 worker 后**立即返回**；`wait:true` 由 IPC 请求线程等待完成 channel，不阻塞控制线程。
  - `Stop` 直接操作 `ProcessRegistry`，不排队到控制线程之后；新增 `ProcessRegistry::stop_all()`。
  - `shutdown`：`stop_all` + 等待子进程回收 + 持久化 interrupted/stopped + join，最后删描述符。
  - 启动时做 **running 状态 reconciliation**：无实际子进程的 `running=true` 改为 stopped/failed 并记 `host-crash/interrupted`。
  - **写放大**：worker 完成后一次事务落状态；调度内部记账不写用户级 audit。
- **验收**：不带 `--wait` 的长任务 run 在合理上限（如 500ms）内返回；运行中 `stop` 有限时间内 kill 子进程；A 长任务运行时 B 仍能触发；删除运行中任务后子进程消失；shutdown 回收全部子进程；崩溃后 stale running 被复位。

### 1.3 🟢 missedPolicy 启动处理顺序（§4.1）
- **改哪里**：`scheduler.rs`(`Scheduler::new`/`run`/`handle_missed_on_start`)。
- **为什么**：`new` 先 `recompute_all_next` 把 interval/calendar/condition 的 `nextRunAt` 推进到未来，随后 `handle_missed_on_start` 已看不到逾期任务，missedPolicy 仅对 once 生效。
- **改动方向**：启动顺序改为：加载持久化 `nextRunAt` → 识别 overdue → 按 missedPolicy 更新 `missedCount/lastMissedAt` 并 skip 或最多补一次 → **最后**重算 `nextRunAt`。
- **验收**：为 interval/calendar/condition 分别构造过去的 `nextRunAt`，覆盖 skip 与 run-once。

### 1.4 🟢 进程输出内存上限（§4.3）
- **改哪里**：`exec.rs`(`read_pipe`/`ProcessRegistry::run`)。
- **为什么**：`read_to_end` 把 stdout/stderr 全量读入内存，64 KiB 截断发生在进程结束后，失控脚本可致 OOM。
- **改动方向**：读取线程只保留固定大小缓冲，超限后继续 drain 管道但不再扩容并标 `truncated=true`；必要时设总上限并终止进程。
- **验收**：大输出脚本下内存与持久化均不超上限。

---

## 批次二：Host / data-dir 边界与 CLI 契约

### 2.1 🟡 自定义 `--data-dir` 的 Hidden Host（§3.2）
- **改哪里**：`cli.rs`(`Dispatch`/`dispatch`)、`lib.rs`(`run_desktop_app`/`init_host_core`/`data_dir`)。
- **为什么**：`Dispatch::HiddenHost` 不携带 data_dir，`dispatch` 丢弃 `--data-dir`，Host 用 exe 同级默认目录启动，launcher 却在自定义目录等 `host.json`，导致 `HOST_NOT_READY`。
- **改动方向**：`Dispatch::HiddenHost{ data_dir }` 携带启动上下文；`run_desktop_app`/`init_host_core` 显式接收 data dir，不再由 AppHandle 推断；Host 单实例按规范化 data dir 区分，自定义 headless Host 不被默认 GUI 的 single-instance 误拦。
- **验收**：`kxtodo --data-dir <tmp> notify ...` 在 `<tmp>/runtime/host.json` 建描述符、默认目录不变、default 与 custom Host 可并存、响应含 `meta.dataDir`。

### 2.2 🟡 IPC 携带 CLI cwd + 统一路径解析时机（§3.3 + 补充 E）
- **改哪里**：`ipc.rs`(`IpcRequest`)、`host.rs`(`handle_ipc_request`)、`ops_schedule.rs`(路径规范化)、`exec.rs`(`resolve_program`)。
- **为什么**：`IpcRequest` 无 cwd，Host 硬编码 `cwd=data_dir`，同一 spec 经 standalone 与 IPC 创建时相对路径解析基准不同；`resolve_program` 又在运行时按 Host 的 PATH 解析裸程序名，使"创建时定死路径"的契约失效。
- **改动方向**：`IpcRequest` 增加规范化绝对 `cwd`，CLI 发送真实 cwd，Host 校验后传给 `ExecContext`；**创建时**把相对 source/program/interpreter/workingDirectory 规范化为绝对路径保存，运行时不再依赖 Host PATH/cwd。
- **验收**：同一 spec 经 standalone 与 IPC 创建，落盘的 source/program/workingDirectory 完全一致。

### 2.3 🟢 IPC 大小写与访问控制（§6.2）
- **改哪里**：`ipc.rs`(`endpoint_for`/`validate_request`/socket 绑定)。
- **为什么**：路径 `to_lowercase()` 无条件执行，Linux 大小写敏感会把不同目录当同一 Host；socket 无凭证/token，仅靠端点名不足以作安全边界。
- **改动方向**：大小写折叠仅 `#[cfg(windows)]`；Unix 用用户私有目录 0600 socket 或校验 peer credential，Windows 设当前用户 SID DACL；描述符加高熵 token，Host 校验 token/requestId/data dir/协议/请求大小。
- **验收**：`/data/Foo` 与 `/data/foo` 视为不同 Host；非本用户无法连接。

### 2.4 🟢 format/jq 在业务执行前校验（§3.4）
- **改哪里**：`cli.rs`(`run_cli`/`finish`/`GlobalArgs`)、`render.rs`、`jq.rs`。
- **为什么**：现先执行业务再解析 format、渲染阶段才编译 jq，无效 format/jq 会"资源已创建但输出失败"，Agent 重试可致重复创建。
- **改动方向**：`--format` 改 clap `ValueEnum`（解析期报错）；jq 拆成"预编译 AST"+"求值"，业务执行前完成语法校验；全局参数错误必须保证 revision 与数据文件不变。
- **验收**：无效 format/jq 执行 task add/config set，退出码 2、revision 不变、资源未创建。

---

## 批次三：协议完整性

### 3.1 🟢 读命令返回 revision meta（§4.5）
- **改哪里**：`core.rs`(task get/list/find/tree、schedule get/list/find/logs/status/validate/runtime list、config list/get/path/validate)、`host.rs`(IPC `custom_data_dir` 硬编码)。
- **为什么**：需求要求所有持久化读/写响应标明目标域与 revision（`--if-revision` 的基础），当前读命令未设；IPC 侧 `custom_data_dir:false` 硬编码使自定义目录响应缺 `meta.dataDir`。
- **改动方向**：加统一 helper 为读命令写 `revisionDomain/revision`；IPC 按实际 data dir 设置 `custom_data_dir`。
- **验收**：所有 read 命令 meta 含 revision；自定义目录响应含 dataDir。

### 3.2 🟢 幂等作用域 + 全命令透传（§4.6）
- **改哪里**：`repo.rs`(`prepare_write`/`finalize_meta`)、`core.rs`(schedule enable/disable、runtime detect/set、config reset 等)。
- **为什么**：幂等仅按 `key` 匹配（`command` 存而不用），同域内 add/modify 复用 key 会误重放；部分写命令向 repo 传 `None` 静默忽略幂等键。
- **改动方向**：幂等作用域改 `(domain, command, key)`；所有接受幂等参数的写命令透传；不支持幂等的命令在参数层明确拒绝。
- **验收**：同域不同 command 复用 key 不误重放。

### 3.3 🟢 audit 失败不使命令失败（§4.7）
- **改哪里**：`repo.rs`(`write_data/write_settings/write_schedule` 中 `audit(...)?`)。
- **为什么**：数据已原子落盘后 audit 失败会让命令返回错误，调用方无法判断是否该重试。
- **改动方向**：audit 属可观测性，失败改为返回成功 + `meta.warnings` 标注，不上抛。
- **验收**：audit 目录不可写时，业务写入仍成功并带 warning。

### 3.4 🟡 task find / schedule list·find 时间过滤（§5.1）
- **改哪里**：`cli.rs`(`TaskFindArgs`/`ScheduleListArgs`/`ScheduleFindArgs`)、schema、帮助。
- **为什么**：`TaskFindArgs` 缺 created/updated/changed/planned/due/completed 的 from/to；schedule list/find 缺 createdAt/updatedAt/lastRunAt/nextRunAt 范围。core 的 `build_item_filter` 已具能力，仅差 flag。
- **改动方向**：补齐 CLI flags、schema、过滤实现、帮助与测试。
- **验收**：按时间范围过滤返回正确子集。

### 3.5 🟢 doctor：支持 `--check` 且只诊断不删文件（§5.2 + §5.5）
- **改哪里**：`cli.rs`(`build_invocation` 对 doctor)、`doctor.rs`(`run_doctor`)。
- **为什么**：`--check` 被解析但忽略，恒跑全部；`run_doctor` 无条件 `cleanup_temp_files` 删文件却自称只读。
- **改动方向**：传入 `{check}` 建可枚举检查 registry，未知检查名返回结构化错误；doctor 只**列出**残留临时文件，清理另设显式 high-risk-write 命令并需确认。
- **验收**：`doctor --check runtimes` 只跑该项；doctor 不修改任何文件。

### 3.6 🟢 schedule status 补全（§5.3）
- **改哪里**：`core.rs`(schedule status)。
- **为什么**：缺 `lastMissedAt`、开机恢复是否可用、自定义 data-dir 不自动注册开机启动的限制、修复提示。
- **改动方向**：定义明确的 status 响应结构并补契约测试。

### 3.7 🟢 SKILL 保持嵌入 + 新增导出子命令（§5.4，产品已定：**必须嵌入**）
- **产品决策**：SKILL **必须编译期嵌入二进制**，打包出的 exe 完全干净、无任何外挂依赖。原 review §5.4 提出的"改为外置文件"作废；`skills.rs` 现有 `include_str!` 嵌入、`skills read/list/validate` 走嵌入内容的做法**保留**。
- **改哪里**：`skills.rs`、`cli.rs`(skills 子命令树)、`core.rs`/渲染路径（echo 需绕过 JSON 信封直出正文）；`docs/kxtodo-v9-cli-requirements.md` §3.7（消除文档冲突，见下）。
- **需求文档同步**：需求 §3.7 当前写"运行时读取 `<exe-dir>/skills/kxtodo/SKILL.md`、不得硬编码进 Rust"，与产品决策冲突，须改写为"SKILL 编译期嵌入二进制、发布 exe 自包含；通过 `skills persist` 按需落地到外部路径"。
- **新增两个子命令**（风格对齐现有 `skills list/read/path/validate`）：
  - `kxtodo skills persist <skill-name> <path>`（Risk: write）
    - 作用：把嵌入的 SKILL 落地到磁盘，自动生成 `<解析目录>/<skill-name>/SKILL.md`。
    - 路径规则：若 `<path>` 末段已是 `skills` 目录则直接用作根，否则在其后补一层 `skills`；随后追加 `<skill-name>/SKILL.md`。即 `path=/a/b` → `/a/b/skills/kxtodo/SKILL.md`；`path=/a/b/skills` → `/a/b/skills/kxtodo/SKILL.md`。
    - 行为：递归建目录、写入嵌入正文（存在则覆盖）；返回 envelope，`data` 含最终绝对路径。
    - 说明：命名沿用现有动词风格用 `persist`（用户原文 `persistant` 为拼写，如需保留原词可加 `persistant` 作隐藏别名）。
  - `kxtodo skills echo <skill-name>`（Risk: read）
    - 作用：把 SKILL.md **完整原文直接打印到 stdout**（不包 JSON 信封），便于 `kxtodo skills echo kxtodo > SKILL.md`。
    - 行为：成功时 stdout 为纯 Markdown 正文、退出码 0；未知 skill-name 仍走标准错误信封到 stderr。
- **验收**：`skills persist kxtodo /tmp/x` 生成 `/tmp/x/skills/kxtodo/SKILL.md`，内容与嵌入一致；传 `/tmp/x/skills` 不重复补 `skills`；`skills echo kxtodo` 输出的正文可被 `skills validate` 通过；打包 exe 无外部 SKILL 依赖仍可 `skills read/echo/persist`。

### 3.8 🟢 clap 参数错误包装为 JSON envelope（§5.6）
- **改哪里**：`cli.rs`(参数错误/未知命令/互斥冲突分支)。
- **为什么**：现直接 `error.to_string()`，非标准错误信封。
- **改动方向**：help/version 保留纯文本；参数错误统一包成 envelope（含稳定 code + requestId），测试断言 stderr 可解析为 JSON。

### 3.9 🟢 级联删除附属文件的持久恢复记录（§6.3）
- **改哪里**：`core.rs`(级联删除流程，约 backup→提交 data.json→删图片目录一段)。
- **为什么**：当前 backup 在 Repository 写事务锁外创建；data.json 删除提交后再删图片目录，失败仅写本次响应 warning——业务 JSON 已删、图片操作不可回滚，调用方丢失输出即无法判断是否存在半完成事务。
- **改动方向**：backup 与业务写入纳入同一事务边界；用持久 recovery record/outbox 记待删图片目录；doctor 检查并报告恢复记录，清理完成后删除该记录；幂等重放不得重复建 backup 或误处理已删资源。
- **验收**：删图片目录失败后重启，doctor 能报出未完成的恢复记录并可继续清理。

---

## 批次四：前端联动与补充项

### 4.1 🟢 事件刷新 pending 合并（§6.1）
- **改哪里**：`src/lib/stores.ts`(`refreshFromCore`)。
- **为什么**：`snapshotInFlight` 期间到达的事件被直接丢弃，可能永久漏掉一次刷新，违反 1 秒联动。
- **改动方向**：维护 pending domains，in-flight 时合并，当前请求结束后立即再刷新；最好比较事件 revision 与 snapshot revision。

### 4.2 🟢 通知 `--duration` 静默钳制（补充 A）
- **改哪里**：`host.rs`(`resolve_notification_payload` 的 `clamp(1_200,60_000)`)。
- **为什么**：超范围时长被静默改写，无报错无提示。
- **改动方向**：超范围返回校验错误，或在 `meta.warnings` 标注实际生效值。

### 4.3 🟢 无效枚举报错（补充 C）
- **改哪里**：`core.rs`(`build_item_filter` 的 sort/status 兜底)。
- **为什么**：拼错 `--sort`/`--status` 静默回退默认值，Agent 难自查。
- **改动方向**：未知枚举值返回退出码 2。

### 4.4 🟢 recompute 计划错误改为诊断（补充 D）
- **改哪里**：`scheduler.rs`(`recompute_all_next` 的 `Err(_)=>enabled=false`)。
- **为什么**：任何计划错误（含瞬时）都静默永久禁用任务且无报告。
- **改动方向**：区分永久性 spec 错误与可重试错误，写结构化诊断，不静默关闭。

---

## 批次五：结构、格式与门禁（收尾）

### 5.1 🟢 rustfmt / 版本一致（§7.3 + §7.4）
- `cargo fmt --all` 统一格式；同步 `package-lock.json`（当前根版本仍 8.3.2）与 package/Cargo/Tauri 到 9.0.0。

### 5.2 🟡 CI 门禁（§7.5）
- 生成物重生成后检查无差异；跑所有 help/schema/SKILL 示例；`skills validate`；`cargo test`；前端 check/build。作为合并门禁。

### 5.3 🟡 legacy 收敛（§7.1 + §7.2，可 v9 后继续）
- 将 `lib.rs` 的 `save_*`、旧调度执行器、`SchedulerProcessState` 等拆为 mobile-only；桌面不注册全量保存命令。`core.rs` 按 task/schedule/config 拆 handlers。**不阻塞本版**。

---

## 关键测试补齐（贯穿各批次）

调度：不带 `--wait` 的 run 时延、运行中 stop kill 子进程、删除运行中任务回收、并发不互阻、shutdown 回收全部、崩溃后 stale running 恢复、interval/calendar/condition 的 missedPolicy、大输出上限。
边界/契约：custom data-dir Hidden Host 真实子进程测试、IPC 相对路径与 standalone 一致、无效 format/jq 不写入、所有 read 命令 meta 含 revision、跨命令复用幂等键、audit 失败提交契约、doctor 不改文件、`doctor --check` 只跑指定项、clap 错误为 JSON envelope。
前端：GUI 全部 `gui.*` bridge 命令、ITEM_CONFLICT stale updatedAt、domain-changed 连发不丢刷新、数据损坏时绝不进入桌面 legacy 写。
SKILL/一致性：`skills persist` 路径解析（补/不补 `skills`）与落盘内容一致、`skills echo` 纯正文可管道重定向、级联删除失败后 doctor 报出恢复记录。

**需修正的现有测试用例（§8.3）**：
- `condition_probe_gates_main_action`：当前用 `NOT_READY` 匹配 `READY` 且接受"可能触发"，未真正验证不匹配时主动作不执行。改为构造明确不含 pattern 的输出，强断言通知数=0、runCount 不增、probe 状态 failed。
- `run_now_manual_returns_output`：手动运行是否计 runCount 属需求未定义项，先做产品语义澄清，再统一需求/代码/测试，勿在测试中固化未定行为。

---

## 建议节奏

- **迭代 1（阻断项）**：批次一全部 + 2.1/2.2/2.4。
- **迭代 2（契约补全）**：批次二剩余 + 批次三。
- **迭代 3（联动/收尾）**：批次四 + 批次五。
- 每迭代结束在目标 Windows 环境跑一次完整 `cargo test` + 打包验证。
