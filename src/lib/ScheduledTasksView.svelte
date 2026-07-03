<script lang="ts">
  import {
    Check, Clock3, FileCode2, FolderOpen, Plus, Power, Settings, Trash2
  } from "@lucide/svelte";
  import { appState, commitScheduler, now, showToast } from "./stores";
  import {
    createScheduledTask,
    defaultScheduledTaskAction,
    defaultScheduledTaskTrigger,
    schedulerRuntimeKeys
  } from "./defaults";
  import { pickExecutableFile, resolveExecutorPaths } from "./backend";
  import Dropdown from "./Dropdown.svelte";
  import SchedulerActionEditor from "./SchedulerActionEditor.svelte";
  import type {
    ScheduledTask,
    ScheduledTaskAction,
    ScheduledTaskTrigger,
    SchedulerCondition,
    SchedulerRuntimeKey
  } from "./types";

  const runtimeLabels: Record<SchedulerRuntimeKey, string> = {
    python: "Python",
    node: "Node.js",
    pwsh: "PowerShell",
    bash: "Bash",
    make: "Make"
  };

  const languageLabels: Record<ScheduledTaskAction["language"], string> = {
    python: "Python",
    javascript: "JavaScript / Node.js",
    powershell: "PowerShell",
    bash: "Bash",
    makefile: "Makefile",
    custom: "自定义"
  };

  const triggerOptions: Array<{ value: ScheduledTaskTrigger["type"]; label: string }> = [
    { value: "once", label: "指定时间触发一次" },
    { value: "interval", label: "每隔一定时间触发" },
    { value: "calendar", label: "按日历 / Cron 触发" },
    { value: "condition", label: "满足条件时触发" }
  ];

  const actionTypeOptions: Array<{ value: ScheduledTaskAction["type"]; label: string }> = [
    { value: "script", label: "执行脚本" },
    { value: "executable", label: "执行可执行文件" }
  ];

  const conditionModeOptions: Array<{ value: SchedulerCondition["mode"]; label: string }> = [
    { value: "contains", label: "包含文本" },
    { value: "regex", label: "匹配正则" }
  ];

  let showRuntimeSettings = false;

  $: scheduledTasks = $appState.scheduler.tasks;

  export function toggleRuntimeSettings(): void {
    showRuntimeSettings = !showRuntimeSettings;
  }

  export function closeOverlays(): void {
    showRuntimeSettings = false;
  }

  function addScheduledTask(): void {
    const task = createScheduledTask(`定时任务 ${scheduledTasks.length + 1}`);
    commitScheduler({
      ...$appState.scheduler,
      tasks: [...scheduledTasks, task]
    });
  }

  function updateTask(taskId: string, updater: (task: ScheduledTask) => ScheduledTask): void {
    commitScheduler({
      ...$appState.scheduler,
      tasks: scheduledTasks.map((task) => task.id === taskId ? { ...updater(task), updatedAt: now() } : task)
    });
  }

  function patchTask(taskId: string, patch: Partial<ScheduledTask>): void {
    updateTask(taskId, (task) => ({ ...task, ...patch }));
  }

  function patchAction(taskId: string, patch: Partial<ScheduledTaskAction>): void {
    updateTask(taskId, (task) => ({ ...task, action: { ...task.action, ...patch } }));
  }

  function patchTrigger(taskId: string, patch: Partial<ScheduledTaskTrigger>): void {
    updateTask(taskId, (task) => ({ ...task, trigger: { ...task.trigger, ...patch } }));
  }

  function patchStopCondition(taskId: string, patch: Partial<SchedulerCondition>): void {
    updateTask(taskId, (task) => ({
      ...task,
      trigger: {
        ...task.trigger,
        stopCondition: { ...task.trigger.stopCondition, ...patch }
      }
    }));
  }

  function patchProbeCondition(taskId: string, patch: Partial<SchedulerCondition>): void {
    updateTask(taskId, (task) => ({
      ...task,
      trigger: {
        ...task.trigger,
        probeCondition: { ...task.trigger.probeCondition, ...patch }
      }
    }));
  }

  function patchProbeAction(taskId: string, patch: Partial<ScheduledTaskAction>): void {
    updateTask(taskId, (task) => ({
      ...task,
      trigger: {
        ...task.trigger,
        probeAction: { ...task.trigger.probeAction, ...patch }
      }
    }));
  }

  function setTriggerType(taskId: string, type: ScheduledTaskTrigger["type"]): void {
    updateTask(taskId, (task) => ({
      ...task,
      trigger: {
        ...defaultScheduledTaskTrigger(type),
        type,
        stopCondition: task.trigger.stopCondition,
        probeCondition: task.trigger.probeCondition,
        probeAction: task.trigger.probeAction
      }
    }));
  }

  function setActionType(taskId: string, type: ScheduledTaskAction["type"]): void {
    updateTask(taskId, (task) => ({
      ...task,
      action: type === "script"
        ? { ...defaultScheduledTaskAction(task.action.language), type: "script" }
        : { ...task.action, type: "executable" }
    }));
  }

  function setActionLanguage(taskId: string, language: ScheduledTaskAction["language"]): void {
    updateTask(taskId, (task) => ({
      ...task,
      action: {
        ...task.action,
        language,
        interpreter: "",
        code: task.action.code || defaultScheduledTaskAction(language).code
      }
    }));
  }

  function toggleEnabled(task: ScheduledTask): void {
    const enabled = !task.enabled;
    patchTask(task.id, {
      enabled,
      runCount: enabled ? 0 : task.runCount,
      lastRunAt: enabled ? undefined : task.lastRunAt,
      nextRunAt: enabled && task.trigger.type === "once" ? task.trigger.runAt : task.nextRunAt,
      lastStatus: enabled ? "idle" : task.lastStatus
    });
  }

  function toggleExpanded(task: ScheduledTask): void {
    if (task.editing) {
      return;
    }
    patchTask(task.id, { expanded: !task.expanded });
  }

  function startEditing(taskId: string): void {
    patchTask(taskId, { editing: true, expanded: true });
  }

  function finishEditing(taskId: string): void {
    patchTask(taskId, { editing: false, expanded: false });
  }

  function toggleEditing(task: ScheduledTask): void {
    if (task.editing) {
      finishEditing(task.id);
    } else {
      startEditing(task.id);
    }
  }

  function deleteTask(taskId: string): void {
    commitScheduler({
      ...$appState.scheduler,
      tasks: scheduledTasks.filter((task) => task.id !== taskId)
    });
  }

  function updateRuntime(key: SchedulerRuntimeKey, value: string): void {
    commitScheduler({
      ...$appState.scheduler,
      runtimes: {
        ...$appState.scheduler.runtimes,
        [key]: value
      }
    });
  }

  async function refreshRuntimes(): Promise<void> {
    try {
      const resolved = await resolveExecutorPaths();
      commitScheduler({
        ...$appState.scheduler,
        runtimes: {
          ...$appState.scheduler.runtimes,
          ...Object.fromEntries(schedulerRuntimeKeys.map((key) => [key, $appState.scheduler.runtimes[key] || resolved[key] || ""]))
        } as Record<SchedulerRuntimeKey, string>
      });
      showToast("已从环境变量刷新执行器路径");
    } catch (error) {
      showToast(`刷新执行器路径失败：${String(error)}`);
    }
  }

  async function browseRuntime(key: SchedulerRuntimeKey): Promise<void> {
    try {
      const path = await pickExecutableFile();
      if (path) {
        updateRuntime(key, path);
      }
    } catch (error) {
      showToast(`选择执行器失败：${String(error)}`);
    }
  }

  function clearOutput(taskId: string): void {
    patchTask(taskId, {
      lastExitCode: undefined,
      lastStdout: "",
      lastStderr: ""
    });
  }

  function textValue(event: Event): string {
    const target = event.currentTarget;
    return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement ? target.value : "";
  }

  function numberValue(event: Event, fallback: number, min: number, max: number): number {
    const value = Number(textValue(event));
    if (!Number.isFinite(value)) {
      return fallback;
    }
    return Math.min(max, Math.max(min, Math.round(value)));
  }

  function checkedValue(event: Event): boolean {
    const target = event.currentTarget;
    return target instanceof HTMLInputElement ? target.checked : false;
  }

  function runtimePlaceholder(language: ScheduledTaskAction["language"]): string {
    const key = runtimeKeyForLanguage(language);
    return key ? $appState.scheduler.runtimes[key] || "未从环境变量找到" : "输入自定义执行器路径";
  }

  function runtimeKeyForLanguage(language: ScheduledTaskAction["language"]): SchedulerRuntimeKey | null {
    if (language === "python") return "python";
    if (language === "javascript") return "node";
    if (language === "powershell") return "pwsh";
    if (language === "bash") return "bash";
    if (language === "makefile") return "make";
    return null;
  }

  function describeTrigger(task: ScheduledTask): string {
    if (task.trigger.type === "once") return `一次 · ${formatDateTime(task.trigger.runAt)}`;
    if (task.trigger.type === "interval") {
      const repeats = task.trigger.repeatCount === 0 ? "无限次" : `${task.runCount}/${task.trigger.repeatCount} 次`;
      return `每 ${task.trigger.everySeconds} 秒 · ${repeats}`;
    }
    if (task.trigger.type === "calendar") return `Cron · ${task.trigger.cron}`;
    return `条件轮询 · 每 ${task.trigger.everySeconds} 秒`;
  }

  function describeAction(action: ScheduledTaskAction): string {
    if (action.type === "executable") return action.executablePath || "可执行文件未配置";
    const source = action.scriptMode === "path" ? action.filePath || "脚本路径未配置" : "内联代码";
    return `${languageLabels[action.language]} · ${source}`;
  }

  function formatDateTime(value?: string): string {
    if (!value) return "未设置";
    const date = new Date(value);
    if (Number.isNaN(date.getTime())) return value;
    return `${date.getMonth() + 1}月${date.getDate()}日 ${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
  }

  function statusLabel(task: ScheduledTask): string {
    if (task.lastStatus === "running") return "执行中";
    if (task.enabled) return "调度中";
    if (task.lastStatus === "stopped") return "已停止";
    return "未启用";
  }

  function statusTone(task: ScheduledTask): "idle" | "scheduled" | "running" | "stopped" {
    if (task.lastStatus === "running") return "running";
    if (task.enabled) return "scheduled";
    if (task.lastStatus === "stopped") return "stopped";
    return "idle";
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<section class="scheduler-panel" on:click|stopPropagation>
  {#if showRuntimeSettings}
    <section class="runtime-settings">
      <div class="runtime-settings-title">
        <strong>选择执行器</strong>
        <span>留空时会继续使用环境变量 / PATH 中解析到的默认路径。</span>
      </div>
      {#each schedulerRuntimeKeys as key}
        <div class="runtime-path-row">
          <label>
            <span>{runtimeLabels[key]}</span>
            <input
              value={$appState.scheduler.runtimes[key]}
              placeholder={`自动解析 ${runtimeLabels[key]} 路径`}
              on:input={(event) => updateRuntime(key, textValue(event))}
            />
          </label>
          <button type="button" title="选择可执行文件" on:click={() => void browseRuntime(key)}>
            <FolderOpen size={17} />
            选择
          </button>
        </div>
      {/each}
      <button class="runtime-refresh" type="button" on:click={refreshRuntimes}>从环境变量刷新</button>
    </section>
  {/if}

  <section class="scheduled-list">
    {#each scheduledTasks as task (task.id)}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <article
        class="scheduled-card"
        class:compact={!task.expanded && !task.editing}
        class:expanded={task.expanded && !task.editing}
        class:editing={task.editing}
        class:disabled={!task.enabled}
        class:running={task.lastStatus === "running"}
        on:click={() => toggleExpanded(task)}
      >
        <div class="scheduled-card-head">
          <span class="scheduled-clock"><Clock3 size={20} /></span>
          <button class="scheduled-title-row" type="button" on:click|stopPropagation={() => toggleExpanded(task)}>
            <strong>{task.name}</strong>
            <span>{describeTrigger(task)}</span>
          </button>

          <span class={`scheduler-status ${statusTone(task)}`}>{statusLabel(task)}</span>

          <button class="scheduler-power" class:enabled={task.enabled} class:disabled={!task.enabled} type="button" title={task.enabled ? "停用" : "启用"} on:click|stopPropagation={() => toggleEnabled(task)}>
            <Power size={18} />
          </button>

          <button class="scheduler-config-button" type="button" title={task.editing ? "完成配置" : "配置定时任务"} on:click|stopPropagation={() => toggleEditing(task)}>
            {#if task.editing}<Check size={18} />{:else}<Settings size={18} />{/if}
          </button>

          <button class="scheduler-delete" type="button" title="删除" on:click|stopPropagation={() => deleteTask(task.id)}>
            <Trash2 size={17} />
          </button>
        </div>

        {#if task.editing}
          <div class="scheduled-card-panel scheduler-editor" on:click|stopPropagation>
            <label class="wide">
              <span>任务名称</span>
              <input value={task.name} on:input={(event) => patchTask(task.id, { name: textValue(event) || "未命名定时任务" })} />
            </label>

            <div class="scheduler-form-grid">
              <label>
                <span>触发类型</span>
                <Dropdown
                  value={task.trigger.type}
                  options={triggerOptions}
                  ariaLabel="触发类型"
                  on:change={(event) => setTriggerType(task.id, event.detail as ScheduledTaskTrigger["type"])}
                />
              </label>
              <label>
                <span>动作类型</span>
                <Dropdown
                  value={task.action.type}
                  options={actionTypeOptions}
                  ariaLabel="动作类型"
                  on:change={(event) => setActionType(task.id, event.detail as ScheduledTaskAction["type"])}
                />
              </label>
            </div>

            {#if task.trigger.type === "once"}
              <label class="wide">
                <span>触发时间</span>
                <input type="datetime-local" value={task.trigger.runAt} on:input={(event) => patchTrigger(task.id, { runAt: textValue(event) })} />
              </label>
            {:else if task.trigger.type === "interval"}
              <div class="scheduler-form-grid">
                <label>
                  <span>间隔秒数</span>
                  <input type="number" min="1" value={task.trigger.everySeconds} on:input={(event) => patchTrigger(task.id, { everySeconds: numberValue(event, 300, 1, 31536000) })} />
                </label>
                <label>
                  <span>重复次数（0 = 无限）</span>
                  <input type="number" min="0" value={task.trigger.repeatCount} on:input={(event) => patchTrigger(task.id, { repeatCount: numberValue(event, 0, 0, 1000000) })} />
                </label>
              </div>
              <div class="condition-row">
                <label class="checkbox-line">
                  <input type="checkbox" checked={task.trigger.stopCondition.enabled} on:change={(event) => patchStopCondition(task.id, { enabled: checkedValue(event) })} />
                  stdout 满足条件时终止
                </label>
                <div class="condition-match-row">
                  <div class="condition-mode-select">
                    <Dropdown
                      value={task.trigger.stopCondition.mode}
                      options={conditionModeOptions}
                      ariaLabel="停止条件匹配方式"
                      on:change={(event) => patchStopCondition(task.id, { mode: event.detail as SchedulerCondition["mode"] })}
                    />
                  </div>
                  <input value={task.trigger.stopCondition.pattern} placeholder="例如 DONE 或 ^ok" on:input={(event) => patchStopCondition(task.id, { pattern: textValue(event) })} />
                </div>
              </div>
            {:else if task.trigger.type === "calendar"}
              <label class="wide">
                <span>Cron 表达式（分 时 日 月 周）</span>
                <input value={task.trigger.cron} placeholder="0 9 * * *" on:input={(event) => patchTrigger(task.id, { cron: textValue(event) })} />
                <small>例：每天 9:00 = 0 9 * * *；每周一 8:30 = 30 8 * * 1；每月 1 号 10:00 = 0 10 1 * *</small>
              </label>
            {:else}
              <div class="scheduler-form-grid">
                <label>
                  <span>检查间隔秒数</span>
                  <input type="number" min="1" value={task.trigger.everySeconds} on:input={(event) => patchTrigger(task.id, { everySeconds: numberValue(event, 60, 1, 31536000) })} />
                </label>
                <label>
                  <span>条件判断</span>
                  <Dropdown
                    value={task.trigger.probeCondition.mode}
                    options={conditionModeOptions}
                    ariaLabel="条件判断匹配方式"
                    on:change={(event) => patchProbeCondition(task.id, { mode: event.detail as SchedulerCondition["mode"] })}
                  />
                </label>
              </div>
              <label class="wide">
                <span>触发条件</span>
                <input value={task.trigger.probeCondition.pattern} placeholder="例如 READY 或 ^changed=true" on:input={(event) => patchProbeCondition(task.id, { enabled: true, pattern: textValue(event) })} />
              </label>
              <SchedulerActionEditor
                title="条件检测脚本"
                action={task.trigger.probeAction}
                placeholder={runtimePlaceholder(task.trigger.probeAction.language)}
                onPatch={(patch) => patchProbeAction(task.id, patch)}
              />
            {/if}

            <SchedulerActionEditor
              title="执行动作"
              action={task.action}
              placeholder={runtimePlaceholder(task.action.language)}
              onPatch={(patch) => patchAction(task.id, patch)}
              onType={(type) => setActionType(task.id, type)}
              onLanguage={(language) => setActionLanguage(task.id, language)}
            />
          </div>
        {:else if task.expanded}
          <div class="scheduled-card-panel scheduled-expanded-body" on:click|stopPropagation>
            <strong>{describeAction(task.action)}</strong>
            <div class="scheduled-meta-grid">
              <span>运行次数：{task.runCount}</span>
              <span>上次运行：{formatDateTime(task.lastRunAt)}</span>
              <span>退出码：{task.lastExitCode ?? "—"}</span>
            </div>
            {#if task.lastStdout || task.lastStderr}
              <div class="scheduler-output-head">
                <span>最近一次执行输出</span>
                <button type="button" on:click|stopPropagation={() => clearOutput(task.id)}>清空</button>
              </div>
              <div class="scheduler-output">
                {#if task.lastStdout}<pre>{task.lastStdout}</pre>{/if}
                {#if task.lastStderr}<pre class="stderr">{task.lastStderr}</pre>{/if}
              </div>
            {:else}
              <div class="scheduler-output-empty">暂无 stdout / stderr 输出</div>
            {/if}
          </div>
        {/if}
      </article>
    {/each}

    {#if scheduledTasks.length === 0}
      <div class="empty-state scheduler-empty">
        <FileCode2 size={42} />
        <strong>还没有定时任务</strong>
        <span>点击右下角加号，创建一次性、间隔、Cron 或条件触发任务。</span>
      </div>
    {/if}
  </section>

  <button class="scheduler-floating-add" type="button" title="添加定时任务" on:click={addScheduledTask}>
    <Plus size={28} />
  </button>
</section>
