<script lang="ts">
  import {
    Check, ChevronUp, Clock3, FileCode2, PenLine, Play, Plus, Power, Settings, Trash2
  } from "@lucide/svelte";
  import { appState, commitScheduler, now, showToast } from "./stores";
  import {
    createScheduledTask,
    defaultScheduledTaskAction,
    defaultScheduledTaskTrigger,
    schedulerRuntimeKeys
  } from "./defaults";
  import { resolveExecutorPaths, runScheduledAction } from "./backend";
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

  const triggerLabels: Record<ScheduledTaskTrigger["type"], string> = {
    once: "指定时间触发一次",
    interval: "每隔一定时间触发",
    calendar: "按日历 / Cron 触发",
    condition: "满足条件时触发"
  };

  const statusLabels: Record<ScheduledTask["lastStatus"], string> = {
    idle: "待机",
    running: "运行中",
    success: "成功",
    failed: "失败",
    stopped: "已停止"
  };

  let showRuntimeSettings = false;
  let manualRunningId: string | null = null;

  $: scheduledTasks = $appState.scheduler.tasks;

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
    patchTask(taskId, { editing: false, expanded: true });
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

  async function runNow(task: ScheduledTask): Promise<void> {
    if (manualRunningId) {
      return;
    }
    manualRunningId = task.id;
    patchTask(task.id, { lastStatus: "running" });
    try {
      const output = await runScheduledAction(task.action, $appState.scheduler.runtimes);
      patchTask(task.id, {
        runCount: task.runCount + 1,
        lastRunAt: now(),
        lastStatus: output.exitCode === 0 ? "success" : "failed",
        lastExitCode: output.exitCode,
        lastStdout: output.stdout,
        lastStderr: output.stderr
      });
    } catch (error) {
      patchTask(task.id, {
        lastStatus: "failed",
        lastStderr: String(error)
      });
      showToast(`立即运行失败：${task.name}`);
    } finally {
      manualRunningId = null;
    }
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
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<section class="scheduler-panel" on:click|stopPropagation>
  <div class="scheduler-toolbar">
    <button class="scheduler-gear" type="button" title="执行器路径" on:click={() => (showRuntimeSettings = !showRuntimeSettings)}>
      <Settings size={20} />
    </button>
    <div>
      <strong>脚本执行器</strong>
      <span>Python / PowerShell / Node / Bash / Makefile 的路径会保存到 tasks.json</span>
    </div>
  </div>

  {#if showRuntimeSettings}
    <section class="runtime-settings">
      {#each schedulerRuntimeKeys as key}
        <label>
          <span>{runtimeLabels[key]}</span>
          <input
            value={$appState.scheduler.runtimes[key]}
            placeholder={`自动解析 ${runtimeLabels[key]} 路径`}
            on:input={(event) => updateRuntime(key, textValue(event))}
          />
        </label>
      {/each}
      <button class="runtime-refresh" type="button" on:click={refreshRuntimes}>从环境变量刷新</button>
    </section>
  {/if}

  <section class="scheduled-list">
    {#each scheduledTasks as task (task.id)}
      <article
        class="scheduled-card"
        class:compact={!task.expanded && !task.editing}
        class:expanded={task.expanded && !task.editing}
        class:editing={task.editing}
        class:disabled={!task.enabled}
        class:running={task.lastStatus === "running"}
        on:dblclick|preventDefault={() => toggleExpanded(task)}
      >
        <div class="scheduled-title-grid">
          <span class="scheduled-clock"><Clock3 size={20} /></span>
          <section class="scheduled-body">
            {#if task.editing}
              <div class="scheduler-editor">
                <label class="wide">
                  <span>任务名称</span>
                  <input value={task.name} on:input={(event) => patchTask(task.id, { name: textValue(event) || "未命名定时任务" })} />
                </label>

                <div class="scheduler-form-grid">
                  <label>
                    <span>触发类型</span>
                    <select value={task.trigger.type} on:change={(event) => setTriggerType(task.id, textValue(event) as ScheduledTaskTrigger["type"])}>
                      {#each Object.entries(triggerLabels) as [type, label]}
                        <option value={type}>{label}</option>
                      {/each}
                    </select>
                  </label>
                  <label>
                    <span>动作类型</span>
                    <select value={task.action.type} on:change={(event) => setActionType(task.id, textValue(event) as ScheduledTaskAction["type"])}>
                      <option value="script">执行脚本</option>
                      <option value="executable">执行可执行文件</option>
                    </select>
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
                    <select value={task.trigger.stopCondition.mode} on:change={(event) => patchStopCondition(task.id, { mode: textValue(event) as SchedulerCondition["mode"] })}>
                      <option value="contains">包含文本</option>
                      <option value="regex">匹配正则</option>
                    </select>
                    <input value={task.trigger.stopCondition.pattern} placeholder="例如 DONE 或 ^ok" on:input={(event) => patchStopCondition(task.id, { pattern: textValue(event) })} />
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
                      <select value={task.trigger.probeCondition.mode} on:change={(event) => patchProbeCondition(task.id, { mode: textValue(event) as SchedulerCondition["mode"] })}>
                        <option value="contains">stdout 包含</option>
                        <option value="regex">stdout 匹配正则</option>
                      </select>
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
              <div class="scheduled-expanded-body">
                <strong>{describeTrigger(task)}</strong>
                <span>{describeAction(task.action)}</span>
                <div class="scheduled-meta-grid">
                  <span>运行次数：{task.runCount}</span>
                  <span>上次运行：{formatDateTime(task.lastRunAt)}</span>
                  <span>退出码：{task.lastExitCode ?? "—"}</span>
                </div>
                {#if task.lastStdout || task.lastStderr}
                  <div class="scheduler-output">
                    {#if task.lastStdout}<pre>{task.lastStdout}</pre>{/if}
                    {#if task.lastStderr}<pre class="stderr">{task.lastStderr}</pre>{/if}
                  </div>
                {/if}
              </div>
            {:else}
              <div class="scheduled-title-row">
                <strong>{task.name}</strong>
                <span>{describeTrigger(task)}</span>
              </div>
            {/if}
          </section>

          {#if !task.editing}
            <span class="scheduler-status" class:bad={task.lastStatus === "failed"} class:good={task.lastStatus === "success"}>{statusLabels[task.lastStatus]}</span>
          {/if}

          <button class="scheduler-power" type="button" title={task.enabled ? "暂停" : "启用"} on:click|stopPropagation={() => toggleEnabled(task)}>
            <Power size={18} />
          </button>

          <button class="scheduler-run-now" type="button" title="立即运行" disabled={manualRunningId !== null} on:click|stopPropagation={() => void runNow(task)}>
            <Play size={17} />
          </button>

          {#if task.editing}
            <button class="edit-button scheduler-card-button" type="button" title="完成编辑" on:click|stopPropagation={() => finishEditing(task.id)}>
              <Check size={18} />
            </button>
          {:else}
            <button class="edit-button scheduler-card-button" type="button" title="编辑定时任务" on:click|stopPropagation={() => startEditing(task.id)}>
              <PenLine size={18} />
            </button>
          {/if}

          <button class="scheduler-delete" type="button" title="删除" on:click|stopPropagation={() => deleteTask(task.id)}>
            <Trash2 size={17} />
          </button>

          {#if task.expanded && !task.editing}
            <button class="collapse-button" type="button" title="收起卡片" on:click|stopPropagation={() => toggleExpanded(task)}>
              <ChevronUp size={18} />
            </button>
          {/if}
        </div>
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
