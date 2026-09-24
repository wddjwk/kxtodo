<script lang="ts">
  import Dropdown from "./Dropdown.svelte";
  import NumberField from "./NumberField.svelte";
  import { FolderOpen, Search } from "@lucide/svelte";
  import { pickExecutableFile, resolveExecutablePath } from "./backend";
  import { caps, executablePathPlaceholder } from "./capabilities";
  import { showToast } from "./stores";
  import type { AppNotification, ScheduledTaskAction, SchedulerCondition } from "./types";

  export let title = "执行动作";
  export let action: ScheduledTaskAction;
  export let placeholder = "";
  export let allowNotification = false;
  export let probe = false;
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

  // 移动端只有「发送通知」一种动作：设备上没有 shell、没有可执行文件的路径语义，
  // core 的 ops_schedule::ensure_platform_supported 也会拒。界面给了选项，
  // 用户填完一整套脚本才发现保存不了，那是白填。
  $: actionTypeOptions = caps.desktop
    ? [
        { value: "script" as const, label: "脚本" },
        { value: "executable" as const, label: "可执行文件" },
        ...(allowNotification ? [{ value: "notification" as const, label: "发送通知" }] : [])
      ]
    : [{ value: "notification" as const, label: "发送通知" }];

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

  async function browse(key: "executablePath" | "filePath" | "interpreter"): Promise<void> {
    try { const path = await pickExecutableFile(); if (path) onPatch({ [key]: path }); }
    catch (error) { showToast(`选择文件失败：${String(error)}`); }
  }
  async function resolveExecutable(): Promise<void> {
    try {
      const path = await resolveExecutablePath(action.executablePath);
      if (path) onPatch({ executablePath: path });
      else showToast("未在 PATH 中找到该程序");
    } catch (error) { showToast(`解析程序失败：${String(error)}`); }
  }
  function browseExecutable(): Promise<void> { return browse("executablePath"); }
  function browseScriptFile(): Promise<void> { return browse("filePath"); }
  function browseInterpreter(): Promise<void> { return browse("interpreter"); }
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
        <NumberField
          ariaLabel="通知自动隐藏时长"
          suffix="ms"
          min={1200}
          max={60000}
          value={action.notification.durationMs}
          onCommit={(v) => patchNotification({ durationMs: v })}
        />
      </label>
      <small class="wide">可在消息中使用 {"{taskName}"}，脚本输出变量在执行脚本后通知中可用。</small>
    </div>
  {:else}
    {#if action.type === "executable"}
      <label class="wide">
        <span>可执行文件路径</span>
        <div class="scheduler-path-row">
          <input value={action.executablePath} placeholder={executablePathPlaceholder} on:input={(event) => onPatch({ executablePath: textValue(event) })} />
          <button class="settings-button" type="button" title="选择文件" on:click={browseExecutable}>
            <FolderOpen size={15} />
          </button>
          <button class="settings-button" type="button" title="从 PATH 解析" on:click={resolveExecutable}>
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
          <div class="scheduler-path-row">
            <input value={action.interpreter} placeholder={placeholder} on:input={(event) => onPatch({ interpreter: textValue(event) })} />
            <button class="settings-button" type="button" title="选择文件" on:click={browseInterpreter}>
              <FolderOpen size={15} />
            </button>
          </div>
        </label>
      </div>
      {#if action.scriptMode === "path"}
        <label class="wide">
          <span>脚本文件路径</span>
          <div class="scheduler-path-row">
            <input value={action.filePath} placeholder="D:\scripts\task.py" on:input={(event) => onPatch({ filePath: textValue(event) })} />
            <button class="settings-button" type="button" title="选择文件" on:click={browseScriptFile}>
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

    <details class="scheduler-disclosure">
      <summary>{probe ? "参数、工作目录与超时" : "参数、工作目录、超时与通知"}</summary>
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

    <label>
      <span>执行超时</span>
      <input value={action.timeout ?? ""} placeholder="可选，例如 30s、5m" on:input={(event) => onPatch({ timeout: textValue(event) || undefined })} />
      <small>到达超时会终止子进程。探针建议设置有限超时。</small>
    </label>
    {#if !probe}
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
            <NumberField
              ariaLabel="完成通知自动隐藏时长"
              suffix="ms"
              min={1200}
              max={60000}
              value={action.completionNotification.durationMs}
              onCommit={(v) => patchCompletionNotification({ durationMs: v })}
            />
          </label>
        </div>
      {/if}

      <label class="checkbox-line">
        <input type="checkbox" checked={action.stdoutNotification.enabled} on:change={(event) => setStdoutNotificationEnabled(checkedValue(event))} />
        stdout 满足条件时发送通知
      </label>
      {#if action.stdoutNotification.enabled}
        <Dropdown value={action.stdoutNotification.condition.stream ?? "stdout"}
          options={[{ value: "stdout", label: "stdout" }, { value: "stderr", label: "stderr" }]}
          ariaLabel="输出通知检测流"
          on:change={(event) => patchStdoutCondition({ stream: event.detail as SchedulerCondition["stream"] })} />
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
            <NumberField
              ariaLabel="stdout 通知自动隐藏时长"
              suffix="ms"
              min={1200}
              max={60000}
              value={action.stdoutNotification.notification.durationMs}
              onCommit={(v) => patchStdoutNotification({ durationMs: v })}
            />
          </label>
        </div>
      {/if}
      <small>通知消息支持 {"{stdout}"}、{"{stderr}"}、{"{exitCode}"}、{"{taskName}"} 变量。</small>
    </div>
    {/if}
    </details>
  {/if}
</section>

<style>
  .scheduler-path-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .scheduler-path-row input {
    flex: 1;
    min-width: 0;
  }
  .scheduler-path-row button {
    display: grid;
    place-items: center;
    flex-shrink: 0;
    width: 36px;
    min-height: 36px;
    padding: 0;
  }
</style>
