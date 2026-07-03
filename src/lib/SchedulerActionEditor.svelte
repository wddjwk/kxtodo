<script lang="ts">
  import type { ScheduledTaskAction } from "./types";

  export let title = "执行动作";
  export let action: ScheduledTaskAction;
  export let placeholder = "";
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

  function textValue(event: Event): string {
    const target = event.currentTarget;
    return target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement ? target.value : "";
  }
</script>

<section class="action-editor">
  <div class="action-editor-title">{title}</div>
  <div class="scheduler-form-grid">
    <label>
      <span>类型</span>
      <select value={action.type} on:change={(event) => onType(textValue(event) as ScheduledTaskAction["type"])}>
        <option value="script">脚本</option>
        <option value="executable">可执行文件</option>
      </select>
    </label>
    {#if action.type === "script"}
      <label>
        <span>语言</span>
        <select value={action.language} on:change={(event) => onLanguage(textValue(event) as ScheduledTaskAction["language"])}>
          {#each Object.entries(languageLabels) as [language, label]}
            <option value={language}>{label}</option>
          {/each}
        </select>
      </label>
    {/if}
  </div>

  {#if action.type === "executable"}
    <label class="wide">
      <span>可执行文件路径</span>
      <input value={action.executablePath} placeholder="C:\Tools\demo.exe" on:input={(event) => onPatch({ executablePath: textValue(event) })} />
    </label>
  {:else}
    <div class="scheduler-form-grid">
      <label>
        <span>脚本来源</span>
        <select value={action.scriptMode} on:change={(event) => onPatch({ scriptMode: textValue(event) as ScheduledTaskAction["scriptMode"] })}>
          <option value="inline">直接输入代码</option>
          <option value="path">文件路径</option>
        </select>
      </label>
      <label>
        <span>解释器（可覆盖默认值）</span>
        <input value={action.interpreter} placeholder={placeholder} on:input={(event) => onPatch({ interpreter: textValue(event) })} />
      </label>
    </div>
    {#if action.scriptMode === "path"}
      <label class="wide">
        <span>脚本文件路径</span>
        <input value={action.filePath} placeholder="D:\scripts\task.py" on:input={(event) => onPatch({ filePath: textValue(event) })} />
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
</section>
