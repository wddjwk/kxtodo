<script lang="ts">
  import { CalendarDays, Check, Clock3, FileCode2, FolderOpen, Plus, Power, Repeat2, ScanLine, Settings, Trash2, Play, ChevronDown } from "@lucide/svelte";
  import { appState, showToast } from "./stores";
  import { currentMinute } from "./currentTime";
  import { createBackGuard } from "./platform";
  import { addSchedule, modifySchedule, removeSchedule, setScheduleEnabled, stopSchedule, setScheduleUi,
    setRuntime, detectRuntimes, clearScheduleOutput, runSchedule, loadScheduleHistory } from "./actions";
  import { defaultScheduledTaskAction, defaultScheduledTaskTrigger, defaultSchedulerCondition, schedulerRuntimeKeys } from "./defaults";
  import { pickExecutableFile } from "./backend";
  import { caps } from "./capabilities";
  import Dropdown from "./Dropdown.svelte";
  import SchedulerActionEditor from "./SchedulerActionEditor.svelte";
  import { actionSentence, calendarCron, nextRunPreview, readCalendar, scheduleDirty, scheduleWeekdays, stopSentence, validateScheduleDraft, whenSentence, type CalendarRule } from "./scheduleEditor";
  import type { ScheduledTask, ScheduledTaskAction, ScheduledTaskTrigger, SchedulerCondition, SchedulerGate, SchedulerRuntimeKey, ScheduleHistoryRun } from "./types";

  const runtimeLabels: Record<SchedulerRuntimeKey, string> = { python: "Python", node: "Node.js", pwsh: "PowerShell", bash: "Bash", make: "Make" };
  const triggerTiles = [
    { type: "once" as const, label: "指定时间", detail: "只做一次", icon: Clock3 },
    { type: "interval" as const, label: "间隔重复", detail: "每隔一段时间", icon: Repeat2 },
    { type: "calendar" as const, label: "日历计划", detail: "每天、每周或每月", icon: CalendarDays },
    ...(caps.desktop ? [{ type: "condition" as const, label: "满足条件", detail: "检测结果后执行", icon: ScanLine }] : [])
  ];
  const matchOptions = [{ value: "contains", label: "包含文本" }, { value: "regex", label: "匹配正则" }];
  const streamOptions = [{ value: "stdout", label: "stdout" }, { value: "stderr", label: "stderr" }];
  const calendarOptions = [{ value: "daily", label: "每天" }, { value: "weekly", label: "每周" }, { value: "monthly", label: "每月" }];
  let showRuntimeSettings = false;
  let drafts: Record<string, ScheduledTask> = {};
  let steps: Record<string, number> = {};
  let advanced: Record<string, boolean> = {};
  let busy: Record<string, boolean> = {};
  let errors: Record<string, string> = {};
  let histories: Record<string, ScheduleHistoryRun[]> = {};
  let historyLoading: Record<string, boolean> = {};
  let historyErrors: Record<string, string> = {};
  $: scheduledTasks = $appState.scheduler.tasks;
  $: rows = scheduledTasks.map((saved) => ({ saved, task: drafts[saved.id] ?? saved, step: steps[saved.id] ?? 1,
    dirty: scheduleDirty(drafts[saved.id] ?? saved, saved) }));
  const backGuard = createBackGuard();
  $: backGuard(showRuntimeSettings, () => (showRuntimeSettings = false));
  export function toggleRuntimeSettings(): void { showRuntimeSettings = !showRuntimeSettings; }
  export function closeOverlays(): void { showRuntimeSettings = false; }
  function current(id: string): ScheduledTask { return drafts[id] ?? scheduledTasks.find((task) => task.id === id)!; }
  function update(id: string, patch: Partial<ScheduledTask>): void {
    drafts = { ...drafts, [id]: { ...current(id), ...patch, nextRunAt: undefined } };
    errors = { ...errors, [id]: "" };
  }
  function trigger(id: string, patch: Partial<ScheduledTaskTrigger>): void { update(id, { trigger: { ...current(id).trigger, ...patch } }); }
  function action(id: string, patch: Partial<ScheduledTaskAction>): void { update(id, { action: { ...current(id).action, ...patch } }); }
  function condition(id: string, key: "stopCondition" | "probeCondition", patch: Partial<SchedulerCondition>): void {
    trigger(id, { [key]: { ...current(id).trigger[key], ...patch } });
  }
  function gate(id: string, patch: Partial<SchedulerGate>): void { update(id, { gate: { windows: [], ...current(id).gate, ...patch } }); }
  function gateWindow(id: string, index: number, patch: Partial<SchedulerGate["windows"][number]>): void {
    gate(id, { windows: current(id).gate!.windows.map((window, i) => i === index ? { ...window, ...patch } : window) });
  }
  function toggleWindowDay(id: string, index: number, day: number): void {
    const days = current(id).gate!.windows[index].weekdays ?? [0, 1, 2, 3, 4, 5, 6];
    gateWindow(id, index, { weekdays: days.includes(day) ? days.filter((d) => d !== day) : [...days, day].sort() });
  }
  function chooseTrigger(id: string, type: ScheduledTaskTrigger["type"]): void {
    const old = current(id).trigger;
    if (old.type === type) return;
    update(id, { trigger: { ...defaultScheduledTaskTrigger(type), missedPolicy: old.missedPolicy,
      probeAction: old.probeAction, probeCondition: old.probeCondition, stopCondition: old.stopCondition },
      ...(type === "condition" ? { gate: undefined } : {}) });
  }
  function calendar(id: string, patch: Partial<CalendarRule>): void {
    const rule = { ...readCalendar(current(id).trigger.cron), ...patch };
    try { trigger(id, { cron: calendarCron(rule) }); } catch (error) { showToast((error as Error).message); }
  }
  function openStep(id: string, step: number): void {
    steps = { ...steps, [id]: step };
    void setScheduleUi(id, { editing: true, expanded: true });
  }
  function discard(id: string): void {
    const next = { ...drafts }; delete next[id]; drafts = next;
    errors = { ...errors, [id]: "" };
    void setScheduleUi(id, { editing: false, expanded: false });
  }
  async function save(id: string): Promise<void> {
    const task = current(id);
    const error = validateScheduleDraft(task, caps.desktop);
    if (error) { errors = { ...errors, [id]: error }; return; }
    busy = { ...busy, [id]: true };
    const saved = await modifySchedule(id, task);
    busy = { ...busy, [id]: false };
    if (saved) discard(id);
    else errors = { ...errors, [id]: "保存失败，草稿已保留，请检查输入后重试。" };
  }
  async function trial(id: string): Promise<void> {
    busy = { ...busy, [id]: true };
    await runSchedule(id);
    busy = { ...busy, [id]: false };
    if (histories[id]) await history(id);
  }
  async function history(id: string): Promise<void> {
    historyLoading = { ...historyLoading, [id]: true };
    historyErrors = { ...historyErrors, [id]: "" };
    try { histories = { ...histories, [id]: await loadScheduleHistory(id) }; }
    catch (error) { historyErrors = { ...historyErrors, [id]: `读取历史失败：${String(error)}` }; }
    finally { historyLoading = { ...historyLoading, [id]: false }; }
  }
  async function browseRuntime(key: SchedulerRuntimeKey): Promise<void> {
    try { const path = await pickExecutableFile(); if (path) await setRuntime(key, path); }
    catch (error) { showToast(`选择执行器失败：${String(error)}`); }
  }
  function runtimePlaceholder(a: ScheduledTaskAction): string {
    const key = ({ python: "python", javascript: "node", powershell: "pwsh", bash: "bash", makefile: "make" } as Record<string, SchedulerRuntimeKey>)[a.language];
    return key ? $appState.scheduler.runtimes[key] || "使用 PATH 中的执行器" : "自定义执行器路径";
  }
  function text(event: Event): string { return (event.currentTarget as HTMLInputElement).value; }
  function checked(event: Event): boolean { return (event.currentTarget as HTMLInputElement).checked; }
  function formatTime(raw?: string): string { return raw ? new Date(raw).toLocaleString() : "—"; }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<section class="scheduler-panel" on:click|stopPropagation>
  {#if showRuntimeSettings && caps.desktop}
    <section class="runtime-settings">
      <div class="runtime-settings-title"><strong>执行环境</strong><span>留空时自动使用 PATH 中的执行器。</span></div>
      {#each schedulerRuntimeKeys as key}
        <div class="runtime-path-row">
          <label><span>{runtimeLabels[key]}</span><input value={$appState.scheduler.runtimes[key]} placeholder="自动解析执行器路径" on:change={(e) => void setRuntime(key, text(e))} /></label>
          <button class="settings-button" type="button" title="选择执行器" on:click={() => void browseRuntime(key)}><FolderOpen size={16} />选择</button>
        </div>
      {/each}
      <button class="settings-button runtime-refresh" type="button" on:click={() => void detectRuntimes()}>从环境变量刷新</button>
    </section>
  {/if}
  <section class="scheduled-list">
    {#each rows as { task, saved, step, dirty } (task.id)}
      <article class="scheduled-card" class:running={saved.lastStatus === "running"}>
        <div class="scheduled-card-head">
          <span class="scheduled-clock"><Clock3 size={18} /></span>
          <button class="settings-button scheduled-title-row" type="button" aria-expanded={saved.expanded} on:click={() => !saved.editing && setScheduleUi(task.id, { expanded: !saved.expanded })}>
            <strong>{task.name || "未命名定时任务"}</strong><span>{dirty ? "未保存的更改" : saved.enabled ? "自动调度已开启" : "自动调度未开启"}</span>
          </button>
          <span class="scheduler-status" class:running={saved.lastStatus === "running"} class:scheduled={saved.enabled}>{saved.lastStatus === "running" ? "执行中" : saved.enabled ? "调度中" : "未启用"}</span>
          <button class="settings-button scheduler-power" type="button" disabled={busy[task.id] && saved.lastStatus !== "running"} title={saved.lastStatus === "running" ? "停止执行" : saved.enabled ? "停用" : "启用"} on:click={() => saved.lastStatus === "running" ? stopSchedule(task.id) : setScheduleEnabled(task.id, !saved.enabled)}><Power size={17} /></button>
          <button class="settings-button scheduler-config-button" type="button" disabled={busy[task.id]} title={saved.editing ? "保存配置" : "配置定时任务"} on:click={() => saved.editing ? save(task.id) : openStep(task.id, 1)}>{#if saved.editing}<Check size={17} />{:else}<Settings size={17} />{/if}</button>
          <button class="settings-button danger scheduler-delete" type="button" disabled={busy[task.id]} title="删除任务" on:click={() => void removeSchedule(task.id)}><Trash2 size={17} /></button>
        </div>
        <div class="scheduler-sentence" aria-label="任务计划摘要">
          <button class="settings-button" type="button" on:click={() => openStep(task.id, 1)}>{whenSentence(task)}</button><span>，</span>
          <button class="settings-button" type="button" on:click={() => openStep(task.id, 2)}>{actionSentence(task)}</button><span>，</span>
          <button class="settings-button" type="button" on:click={() => openStep(task.id, 3)}>{stopSentence(task)}</button><span>。</span>
        </div>
        <p class="scheduler-next"><Clock3 size={13} /><span>{task.trigger.type === "condition" ? "下次检查" : "下次触发"} · {nextRunPreview(task, $currentMinute)}{!saved.enabled ? "（启用后生效）" : ""}</span></p>
        {#if saved.editing}
          <form class="scheduled-card-panel scheduler-editor scheduler-guided" on:submit|preventDefault={() => void save(task.id)}>
            <fieldset class="scheduler-draft-fields" disabled={busy[task.id]}>
              {#each [1, 2, 3] as number}
                <section class="scheduler-step" class:scheduler-step-active={step === number}>
                  <button class="settings-button scheduler-step-heading" type="button" aria-expanded={step === number} aria-controls={`scheduler-${task.id}-step-${number}`} on:click={() => steps = { ...steps, [task.id]: number }}>
                    <span class="scheduler-step-number">0{number}</span><strong>{number === 1 ? "何时执行" : number === 2 ? "执行什么" : task.trigger.type === "condition" ? "命中之后" : "何时结束"}</strong>
                    <span class="scheduler-step-summary">{number === 1 ? whenSentence(task) : number === 2 ? actionSentence(task) : stopSentence(task)}</span><ChevronDown size={15} />
                  </button>
                  {#if step === number}
                    <div class="scheduler-step-content" id={`scheduler-${task.id}-step-${number}`}>
                      {#if number === 1}
                        <div class="scheduler-trigger-tiles">
                          {#each triggerTiles as tile}
                            <button class="settings-button scheduler-trigger-tile" class:scheduler-tile-selected={task.trigger.type === tile.type} type="button" aria-pressed={task.trigger.type === tile.type} on:click={() => chooseTrigger(task.id, tile.type)}>
                              <svelte:component this={tile.icon} size={20} /><strong>{tile.label}</strong><span>{tile.detail}</span>
                            </button>
                          {/each}
                        </div>
                        {#if task.trigger.type === "once"}
                          <label><span>触发时间 · 本地时间</span><input required type="datetime-local" value={task.trigger.runAt} on:input={(e) => trigger(task.id, { runAt: text(e) })} /></label>
                        {:else if task.trigger.type === "interval" || task.trigger.type === "condition"}
                          <label><span>{task.trigger.type === "condition" ? "检查间隔（秒）" : "执行间隔（秒）"}</span><input required type="number" min="0.001" step="any" value={task.trigger.everySeconds} on:input={(e) => trigger(task.id, { everySeconds: Number(text(e)) })} /></label>
                          {#if task.trigger.type === "condition"}
                            <div class="condition-match-row">
                              <Dropdown value={task.trigger.probeCondition.mode} options={matchOptions} ariaLabel="条件匹配方式" on:change={(e) => condition(task.id, "probeCondition", { mode: e.detail as SchedulerCondition["mode"] })} />
                              <input aria-label="触发匹配文本" placeholder="例如 READY 或 ^changed=true" value={task.trigger.probeCondition.pattern} on:input={(e) => condition(task.id, "probeCondition", { enabled: true, pattern: text(e) })} />
                            </div>
                            <Dropdown value={task.trigger.probeCondition.stream ?? "stdout"} options={streamOptions} ariaLabel="探针输出流" on:change={(e) => condition(task.id, "probeCondition", { stream: e.detail as SchedulerCondition["stream"] })} />
                            <SchedulerActionEditor title="条件检测" probe={true} action={task.trigger.probeAction} placeholder={runtimePlaceholder(task.trigger.probeAction)} onPatch={(patch) => trigger(task.id, { probeAction: { ...task.trigger.probeAction, ...patch } })} />
                          {/if}
                        {:else}
                          {@const rule = readCalendar(task.trigger.cron)}
                          {#if !advanced[task.id] && rule.mode !== "advanced"}
                            <div class="scheduler-form-grid">
                              <label><span>重复方式</span><Dropdown value={rule.mode} options={calendarOptions} ariaLabel="日历重复方式" on:change={(e) => calendar(task.id, { mode: e.detail as CalendarRule["mode"] })} /></label>
                              <label><span>执行时刻</span><input required type="time" value={rule.time} on:change={(e) => calendar(task.id, { time: text(e) })} /></label>
                            </div>
                            {#if rule.mode === "weekly"}
                              <div class="scheduler-weekdays" aria-label="执行星期">{#each [1, 2, 3, 4, 5, 6, 0] as day}<button class="settings-button" class:scheduler-day-selected={rule.weekdays.includes(day)} type="button" aria-pressed={rule.weekdays.includes(day)} on:click={() => calendar(task.id, { weekdays: rule.weekdays.includes(day) ? rule.weekdays.filter((d) => d !== day) : [...rule.weekdays, day].sort() })}>{scheduleWeekdays[day]}</button>{/each}</div>
                            {:else if rule.mode === "monthly"}
                              <label><span>每月几号</span><input type="number" min="1" max="31" value={rule.day} on:change={(e) => calendar(task.id, { day: Number(text(e)) })} /><small>没有该日期的月份会跳过，不提前到月底。</small></label>
                            {/if}
                          {/if}
                          <details class="scheduler-disclosure" open={advanced[task.id] || rule.mode === "advanced"}>
                            <summary>高级 Cron 与时区</summary>
                            <label><span>Cron 表达式</span><input required value={task.trigger.cron} on:input={(e) => { advanced = { ...advanced, [task.id]: true }; trigger(task.id, { cron: text(e) }); }} /><small>5 段：分 时 日 月 周（0 / 7 为周日）；6 / 7 段保留原生 Cron 语法。</small></label>
                            <label><span>IANA 时区</span><input value={task.trigger.timezone ?? Intl.DateTimeFormat().resolvedOptions().timeZone} placeholder="Asia/Shanghai" on:input={(e) => trigger(task.id, { timezone: text(e) })} /></label>
                            {#if rule.mode === "advanced" || advanced[task.id]}<button class="settings-button" type="button" on:click={() => { calendar(task.id, { mode: "daily" }); advanced = { ...advanced, [task.id]: false }; }}>改用每天计划</button>{/if}
                          </details>
                        {/if}
                        {#if task.trigger.type !== "condition"}
                          <details class="scheduler-disclosure" open={Boolean(task.gate)}>
                            <summary>仅在这些条件下执行</summary>
                            <label class="checkbox-line"><input type="checkbox" checked={Boolean(task.gate)} on:change={(e) => update(task.id, { gate: checked(e) ? { windows: [{ start: "09:00", end: "18:00" }] } : undefined })} />启用执行门控</label>
                            {#if task.gate}
                              <small>到点时检查；不满足则跳过本次，不计入运行次数。跨午夜的窗口归属开始当天。</small>
                              {#each task.gate.windows as window, i}
                                <div class="scheduler-gate-window">
                                  <div class="scheduler-form-grid"><label><span>开始（含）</span><input type="time" required value={window.start} on:input={(e) => gateWindow(task.id, i, { start: text(e) })} /></label><label><span>结束（不含）</span><input type="time" required value={window.end} on:input={(e) => gateWindow(task.id, i, { end: text(e) })} /></label></div>
                                  <div class="scheduler-weekdays">{#each [1, 2, 3, 4, 5, 6, 0] as day}<button class="settings-button" type="button" class:scheduler-day-selected={!window.weekdays || window.weekdays.includes(day)} aria-pressed={!window.weekdays || window.weekdays.includes(day)} on:click={() => toggleWindowDay(task.id, i, day)}>{scheduleWeekdays[day]}</button>{/each}</div>
                                  <button class="settings-button danger" type="button" on:click={() => gate(task.id, { windows: task.gate!.windows.filter((_, index) => i !== index) })}>移除此窗口</button>
                                </div>
                              {/each}
                              <button class="settings-button" type="button" on:click={() => gate(task.id, { windows: [...task.gate!.windows, { start: "09:00", end: "18:00" }] })}>添加时间窗口</button>
                              {#if caps.desktop}
                                <label class="checkbox-line"><input type="checkbox" checked={Boolean(task.gate.probeAction)} on:change={(e) => gate(task.id, checked(e) ? { probeAction: { ...defaultScheduledTaskAction(), timeout: "30s" }, condition: { ...defaultSchedulerCondition(true), pattern: "READY" } } : { probeAction: undefined, condition: undefined })} />同时检查探针输出</label>
                                {#if task.gate.probeAction && task.gate.condition}
                                  <div class="condition-match-row"><Dropdown value={task.gate.condition.mode} options={matchOptions} ariaLabel="门控匹配方式" on:change={(e) => gate(task.id, { condition: { ...task.gate!.condition!, mode: e.detail as SchedulerCondition["mode"] } })} /><input aria-label="门控匹配文本" value={task.gate.condition.pattern} on:input={(e) => gate(task.id, { condition: { ...task.gate!.condition!, pattern: text(e) } })} /></div>
                                  <Dropdown value={task.gate.condition.stream ?? "stdout"} options={streamOptions} ariaLabel="门控输出流" on:change={(e) => gate(task.id, { condition: { ...task.gate!.condition!, stream: e.detail as SchedulerCondition["stream"] } })} />
                                  <SchedulerActionEditor title="执行前探针" probe={true} action={task.gate.probeAction} placeholder={runtimePlaceholder(task.gate.probeAction)} onPatch={(patch) => gate(task.id, { probeAction: { ...task.gate!.probeAction!, ...patch } })} />
                                {/if}
                              {/if}
                            {/if}
                          </details>
                        {/if}
                      {:else if number === 2}
                        <SchedulerActionEditor title="执行动作" action={task.action} placeholder={runtimePlaceholder(task.action)} allowNotification={true} onPatch={(patch) => action(task.id, patch)} />
                      {:else}
                        {#if task.trigger.type === "interval"}
                          <label><span>执行次数（0 = 不限）</span><input type="number" min="0" step="1" value={task.trigger.repeatCount} on:input={(e) => trigger(task.id, { repeatCount: Number(text(e)) })} /></label>
                          <label class="checkbox-line"><input type="checkbox" checked={task.trigger.stopCondition.enabled} on:change={(e) => condition(task.id, "stopCondition", { enabled: checked(e) })} />输出满足条件时停止</label>
                          {#if task.trigger.stopCondition.enabled}
                            <div class="condition-match-row"><Dropdown value={task.trigger.stopCondition.mode} options={matchOptions} ariaLabel="停止匹配方式" on:change={(e) => condition(task.id, "stopCondition", { mode: e.detail as SchedulerCondition["mode"] })} /><input aria-label="停止匹配文本" value={task.trigger.stopCondition.pattern} on:input={(e) => condition(task.id, "stopCondition", { pattern: text(e) })} /></div>
                            <Dropdown value={task.trigger.stopCondition.stream ?? "stdout"} options={streamOptions} ariaLabel="停止条件输出流" on:change={(e) => condition(task.id, "stopCondition", { stream: e.detail as SchedulerCondition["stream"] })} />
                          {/if}
                        {:else if task.trigger.type === "condition"}
                          <div class="scheduler-stop-options">
                            <label class="checkbox-line"><input type="radio" name={`stop-${task.id}`} checked={task.trigger.cooldown === undefined} on:change={() => trigger(task.id, { cooldown: undefined })} />匹配并执行后结束</label>
                            <label class="checkbox-line"><input type="radio" name={`stop-${task.id}`} checked={task.trigger.cooldown !== undefined} on:change={() => trigger(task.id, { cooldown: "5m" })} />冷却后继续检测</label>
                          </div>
                          {#if task.trigger.cooldown !== undefined}<label><span>冷却时长</span><input value={task.trigger.cooldown} placeholder="例如 5m、1h" on:input={(e) => trigger(task.id, { cooldown: text(e) })} /><small>下次检测距本次执行至少为「检查间隔」与「冷却时长」中的较大值。</small></label>{/if}
                        {:else}<p class="scheduler-step-note">{task.trigger.type === "once" ? "执行一次后自动结束。试运行不会消耗这一次机会。" : "按日历持续执行，直到手动停用或到达截止日期。"}</p>{/if}
                        <label><span>截止日期（可选）</span><input type="date" value={task.until ?? ""} on:input={(e) => update(task.id, { until: text(e) || undefined })} /><small>包含所选日期的整天，到次日零点停止；日历计划使用上一步的时区。</small></label>
                      {/if}
                    </div>
                  {/if}
                </section>
              {/each}
              <footer class="scheduler-editor-footer">
                <label class="scheduler-footer-name"><span>任务名称</span><input required value={task.name} on:input={(e) => update(task.id, { name: text(e) })} /></label>
                <label class="checkbox-line"><input type="checkbox" checked={task.enabled} on:change={(e) => update(task.id, { enabled: checked(e) })} />启用</label>
                <div class="scheduler-footer-actions">
                  <button class="settings-button" type="button" disabled={dirty || saved.lastStatus === "running"} title={dirty ? "请先保存，再试运行" : "执行已保存的任务，不改变调度计划"} on:click={() => void trial(task.id)}><Play size={14} />试运行</button>
                  <button class="settings-button" type="button" on:click={() => discard(task.id)}>取消</button>
                  <button class="settings-button primary" type="submit"><Check size={15} />保存</button>
                </div>
              </footer>
            </fieldset>
            {#if errors[task.id]}<p class="scheduler-error" role="alert">{errors[task.id]}</p>{/if}
            {#if busy[task.id]}<p class="scheduler-next" role="status">正在处理…</p>{/if}
          </form>
        {:else if saved.expanded}
          <div class="scheduled-card-panel scheduled-expanded-body">
            <div class="scheduled-meta-grid"><span>自动运行：{saved.runCount} 次</span><span>上次自动运行：{formatTime(saved.lastRunAt)}</span><span>退出码：{saved.lastExitCode ?? "—"}</span></div>
            <div class="scheduler-output-head"><span>最近一次执行输出</span><button class="settings-button" type="button" on:click={() => void clearScheduleOutput(task.id)}>清空</button></div>
            {#if saved.lastStdout || saved.lastStderr}<div class="scheduler-output">{#if saved.lastStdout}<pre>{saved.lastStdout}</pre>{/if}{#if saved.lastStderr}<pre class="stderr">{saved.lastStderr}</pre>{/if}</div>{:else}<div class="scheduler-output-empty">暂无 stdout / stderr 输出</div>{/if}
            <button class="settings-button" type="button" disabled={busy[task.id] || saved.lastStatus === "running"} on:click={() => void trial(task.id)}><Play size={14} />试运行一次 · 不改变计划</button>
          </div>
        {/if}
        {#if saved.expanded}
          <details class="scheduler-disclosure scheduler-history" on:toggle={(e) => { if ((e.currentTarget as HTMLDetailsElement).open) void history(task.id); }}>
            <summary>执行历史</summary>
            {#if historyLoading[task.id]}<p role="status">正在读取…</p>{:else if historyErrors[task.id]}<p role="alert">{historyErrors[task.id]}</p>{:else if !histories[task.id]?.length}<p>暂无执行记录；未命中的条件检查不计入历史。</p>{:else}
              {#each histories[task.id] as run}
                <details class="scheduler-history-run"><summary><time>{formatTime(run.startedAt)}</time><span>{run.kind === "manual" ? "试运行" : run.kind === "probe" ? "探针" : "自动"}</span><strong>{run.status === "success" ? "成功" : run.status === "stopped" ? "已停止" : "失败"}</strong></summary>
                  {#if run.stopReason}<p>{run.stopReason}</p>{/if}<div class="scheduler-output">{#if run.stdout}<pre>{run.stdout}</pre>{/if}{#if run.stderr}<pre class="stderr">{run.stderr}</pre>{/if}</div>
                </details>
              {/each}
            {/if}
          </details>
        {/if}
      </article>
    {/each}
    {#if !scheduledTasks.length}<div class="empty-state scheduler-empty"><FileCode2 size={40} /><strong>让重复的事，按计划发生</strong><span>设定时间、选择动作，再决定何时结束。</span><button class="settings-button" type="button" on:click={() => void addSchedule()}><Plus size={16} />创建第一个计划</button></div>{/if}
  </section>
  <button class="settings-button primary scheduler-floating-add" type="button" title="添加定时任务" on:click={() => void addSchedule()}><Plus size={27} /></button>
</section>
