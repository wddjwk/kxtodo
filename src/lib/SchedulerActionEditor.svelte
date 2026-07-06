<script lang="ts">
  import Dropdown from "./Dropdown.svelte";
  import { FolderOpen, Search } from "@lucide/svelte";
  import { pickExecutableFile, resolveExecutablePath } from "./backend";
  import type { AppNotification, ScheduledTaskAction, SchedulerCondition } from "./types";

  export let title = "执行动作";
  export let action: ScheduledTaskAction;
  export let placeholder = "";
  export let allowNotification = false;
  export let onPatch: (patch: Partial<ScheduledTaskAction>) => void = () => undefined;
  export let onType: (type: ScheduledTaskAction["type"]) => void = (type) => onPatch({ type });
  export let onLanguage: (language: ScheduledTaskAction["language"]) => void = (language) => onPatch({ language });

  const languageLabels: Record<ScheduledTaskAction["language"], string> = {
    python: "Python",
    javascript: "JavaScript / Node.js",
    powershell: "PowerShell",
    bash: "Bash",
    makefile: "Makefile",
    custom: "自定义"
  };

  let actionTypeOptions: Array<{ value: ScheduledTaskAction["type"]; label: string }> = [];

  $: actionTypeOptions = [
    { value: "script", label: "脚本" },
    { value: "executable", label: "可执行文件" },
    ...(allowNotification ? [{ value: "notification" as const, label: "发送通知" }] : [])
  ];

  const languageOptions: Array<{ value: ScheduledTaskAction["language"]; label: string }> = Object.entries(languageLabels).map(([value, label]) => ({
    value: value as ScheduledTaskAction["language"],
    label
  }));

  const scriptModeOptions: Array<{ value: ScheduledTaskAction["scriptMode"]; label: string }> = [
    { value: "inline", label: "直接输入代码" },
    { value: "path", label: "文件路径" }
  ];

  const toneOptions: Array<{ value: AppNotification["tone"]; label: string }> = [
    { value: "info", label: "普通" },
    { value: "success", label: "成功" },
    { value: "warning", label: "警告" },
    { value: "error", label: "错误" }
  ];

  const conditionModeOptions: Array<{ value: SchedulerCondition["mode"]; label: string }> = [
    { value: "contains", label: "包含文本" },
    { value: "regex", label: "匹配正则" }
  ];

  function textValue(event: Event): string {
    const target = event.currentTarget;
    return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement ? target.value : "";
  }

  function numberValue(event: Event, fallback: number): number {
    const value = Number(textValue(event));
    if (!Number.isFinite(value) || value < 0) {
      return fallback;
    }
    return Math.round(value);
  }

  function checkedValue(event: Event): boolean {
    const target = event.currentTarget;
    return target instanceof HTMLInputElement ? target.checked : false;
  }

  function patchNotification(patch: Partial<AppNotification>): void {
    onPatch({ notification: { ...action.notification, ...patch } });
  }

  function patchCompletionNotification(patch: Partial<AppNotification>): void {
    onPatch({ completionNotification: { ...action.completionNotification, ...patch } });
  }

  function patchStdoutCondition(patch: Partial<SchedulerCondition>): void {
    onPatch({
      stdoutNotification: {
        ...action.stdoutNotification,
        condition: { ...action.stdoutNotification.condition, ...patch }
      }
    });
  }

  function patchStdoutNotification(patch: Partial<AppNotification>): void {
    onPatch({
      stdoutNotification: {
        ...action.stdoutNotification,
        notification: { ...action.stdoutNotification.notification, ...patch }
      }
    });
  }

  function setStdoutNotificationEnabled(enabled: boolean): void {
    onPatch({
      stdoutNotification: {
        ...action.stdoutNotification,
        enabled,
        condition: { ...action.stdoutNotification.condition, enabled }
      }
    });
  }

  async function browseExecutable(): Promise<void> {
    const p = await pickExecutableFile();
    if (p) onPatch({ executablePath: p });
  }

  async function resolveExecutable(): Promise<void> {
    const p = await resolveExecutablePath(action.executablePath);
    if (p) onPatch({ executablePath: p });
  }

  async function browseScriptFile(): Promise<void> {
    const p = await pickExecutableFile();
    if (p) onPatch({ filePath: p });
  }

  async function browseInterpreter(): Promise<void> {
    const p = await pickExecutableFile();
    if (p) onPatch({ interpreter: p });
  }
</script>

<section class="action-editor">
  <div class="action-editor-title">{title}</div>
  <div class="scheduler-form-grid">
    <label>
      <span>类型</span>
      <Dropdown
        value={action.type}
        options={actionTypeOptions}
        ariaLabel="动作类型"
        on:change={(event) => onType(event.detail as ScheduledTaskAction["type"])}
      />
    </label>
    {#if action.type === "script"}
      <label>
        <span>语言</span>
        <Dropdown
          value={action.language}
          options={languageOptions}
          ariaLabel="脚本语言"
          on:change={(event) => onLanguage(event.detail as ScheduledTaskAction["language"])}
        />
      </label>
    {/if}
  </div>

  {#if action.type === "notification"}
    <div class="notification-fields">
      <label>
        <span>标题</span>
        <input value={action.notification.title} placeholder="KXToDo" on:input={(event) => patchNotification({ title: textValue(event) })} />
      </label>
      <label>
        <span>样式</span>
        <Dropdown
          value={action.notification.tone}
          options={toneOptions}
          ariaLabel="通知样式"
          on:change={(event) => patchNotification({ tone: event.detail as AppNotification["tone"] })}
        />
      </label>
      <label class="wide">
        <span>消息</span>
        <textarea class="notification-message" value={action.notification.message} on:input={(event) => patchNotification({ message: textValue(event) })}></textarea>
      </label>
      <label>
        <span>自动隐藏</span>
        <input type="text" inputmode="numeric" value={action.notification.durationMs} on:input={(event) => patchNotification({ durationMs: numberValue(event, action.notification.durationMs) })} />
      </label>
      <small class="wide">可在消息中使用 {"{taskName}"}，脚本输出变量在执行脚本后通知中可用。</small>
    </div>
  {:else}
    {#if action.type === "executable"}
      <label class="wide">
        <span>可执行文件路径</span>
        <div class="path-input-row">
          <input value={action.executablePath} placeholder="C:\Tools\demo.exe" on:input={(event) => onPatch({ executablePath: textValue(event) })} />
          <button type="button" title="选择文件" on:click={browseExecutable}>
            <FolderOpen size={15} />
          </button>
          <button type="button" title="从 PATH 解析" on:click={resolveExecutable}>
            <Search size={15} />
          </button>
        </div>
      </label>
    {:else}
      <div class="scheduler-form-grid">
        <label>
          <span>脚本来源</span>
          <Dropdown
            value={action.scriptMode}
            options={scriptModeOptions}
            ariaLabel="脚本来源"
            on:change={(event) => onPatch({ scriptMode: event.detail as ScheduledTaskAction["scriptMode"] })}
          />
        </label>
        <label>
          <span>解释器（可覆盖默认值）</span>
          <div class="path-input-row">
            <input value={action.interpreter} placeholder={placeholder} on:input={(event) => onPatch({ interpreter: textValue(event) })} />
            <button type="button" title="选择文件" on:click={browseInterpreter}>
              <FolderOpen size={15} />
            </button>
          </div>
        </label>
      </div>
      {#if action.scriptMode === "path"}
        <label class="wide">
          <span>脚本文件路径</span>
          <div class="path-input-row">
            <input value={action.filePath} placeholder="D:\scripts\task.py" on:input={(event) => onPatch({ filePath: textValue(event) })} />
            <button type="button" title="选择文件" on:click={browseScriptFile}>
              <FolderOpen size={15} />
            </button>
          </div>
        </label>
      {:else}
        <label class="wide">
          <span>内联代码</span>
          <textarea value={action.code} spellcheck="false" on:input={(event) => onPatch({ code: textValue(event) })}></textarea>
        </label>
      {/if}
    {/if}

    <div class="scheduler-form-grid">
      <label>
        <span>参数</span>
        <input value={action.arguments} placeholder='--name "KXToDo"' on:input={(event) => onPatch({ arguments: textValue(event) })} />
      </label>
      <label>
        <span>工作目录</span>
        <input value={action.workingDirectory} placeholder="可选" on:input={(event) => onPatch({ workingDirectory: textValue(event) })} />
      </label>
    </div>

    <div class="notification-followups">
      <label class="checkbox-line">
        <input type="checkbox" checked={action.notifyOnComplete} on:change={(event) => onPatch({ notifyOnComplete: checkedValue(event) })} />
        执行完成后发送通知
      </label>
      {#if action.notifyOnComplete}
        <div class="notification-fields">
          <label>
            <span>标题</span>
            <input value={action.completionNotification.title} on:input={(event) => patchCompletionNotification({ title: textValue(event) })} />
          </label>
          <label>
            <span>样式</span>
            <Dropdown
              value={action.completionNotification.tone}
              options={toneOptions}
              ariaLabel="完成通知样式"
              on:change={(event) => patchCompletionNotification({ tone: event.detail as AppNotification["tone"] })}
            />
          </label>
          <label class="wide">
            <span>消息</span>
            <textarea class="notification-message" value={action.completionNotification.message} on:input={(event) => patchCompletionNotification({ message: textValue(event) })}></textarea>
          </label>
          <label>
            <span>自动隐藏</span>
            <input type="text" inputmode="numeric" value={action.completionNotification.durationMs} on:input={(event) => patchCompletionNotification({ durationMs: numberValue(event, action.completionNotification.durationMs) })} />
          </label>
        </div>
      {/if}

      <label class="checkbox-line">
        <input type="checkbox" checked={action.stdoutNotification.enabled} on:change={(event) => setStdoutNotificationEnabled(checkedValue(event))} />
        stdout 满足条件时发送通知
      </label>
      {#if action.stdoutNotification.enabled}
        <div class="condition-match-row wide">
          <div class="condition-mode-select">
            <Dropdown
              value={action.stdoutNotification.condition.mode}
              options={conditionModeOptions}
              ariaLabel="stdout 通知匹配方式"
              on:change={(event) => patchStdoutCondition({ mode: event.detail as SchedulerCondition["mode"] })}
            />
          </div>
          <input value={action.stdoutNotification.condition.pattern} placeholder="例如 DONE 或 ^ok" on:input={(event) => patchStdoutCondition({ enabled: true, pattern: textValue(event) })} />
        </div>
        <div class="notification-fields">
          <label>
            <span>标题</span>
            <input value={action.stdoutNotification.notification.title} on:input={(event) => patchStdoutNotification({ title: textValue(event) })} />
          </label>
          <label>
            <span>样式</span>
            <Dropdown
              value={action.stdoutNotification.notification.tone}
              options={toneOptions}
              ariaLabel="stdout 通知样式"
              on:change={(event) => patchStdoutNotification({ tone: event.detail as AppNotification["tone"] })}
            />
          </label>
          <label class="wide">
            <span>消息</span>
            <textarea class="notification-message" value={action.stdoutNotification.notification.message} on:input={(event) => patchStdoutNotification({ message: textValue(event) })}></textarea>
          </label>
          <label>
            <span>自动隐藏</span>
            <input type="text" inputmode="numeric" value={action.stdoutNotification.notification.durationMs} on:input={(event) => patchStdoutNotification({ durationMs: numberValue(event, action.stdoutNotification.notification.durationMs) })} />
          </label>
        </div>
      {/if}
      <small>通知消息支持 {"{stdout}"}、{"{stderr}"}、{"{exitCode}"}、{"{taskName}"} 变量。</small>
    </div>
  {/if}
</section>

<style>
  .path-input-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .path-input-row input {
    flex: 1;
    min-width: 0;
  }
  .path-input-row button {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 32px;
    height: 32px;
    border: 1px solid #e0e0e0;
    border-radius: 7px;
    background: #f7f7f7;
    color: #5f6368;
    cursor: pointer;
  }
  .path-input-row button:hover {
    background: #eee;
  }
</style>
