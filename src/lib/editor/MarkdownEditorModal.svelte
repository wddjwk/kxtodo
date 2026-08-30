<script lang="ts">
  import { onMount, tick } from "svelte";
  import { Check, Eye, ImagePlus, PenLine, X } from "@lucide/svelte";
  import type { EditorView } from "@codemirror/view";
  import { createMarkdownEditor, insertAtCursor } from "./codemirrorSetup";
  import { hasMultipleMarkdownLines, markdownTitle, renderMarkdown } from "../markdown";
  import { mdImageCache, primeMdImageCache, resolveMarkdownImages } from "../images";
  import {
    isTauriRuntime, mdImageUrl, pickImageFile, saveMdImage, saveMdImageFromDataUrl
  } from "../backend";
  import { appState, clearEditBase, markEditStart, showToast } from "../stores";
  import { saveTaskMarkdown } from "../actions";

  export let taskId: string;
  export let onClose: () => void = () => {};
  export let onOpenLink: (url: string) => void = () => {};

  let host: HTMLDivElement;
  let view: EditorView | null = null;
  let mode: "edit" | "preview" = "edit";
  let text = "";
  let initialText = "";
  let saving = false;
  let title = "编辑任务";

  $: task = $appState.tasks.find((item) => item.id === taskId);
  $: if (!task) {
    onClose();
  }
  $: nodeId = task?.nodeId ?? "";
  // 预览惰性渲染：编辑态不做全量 markdown+高亮（长文档逐键全量渲染会卡死主线程）
  let previewHtml = "";
  $: if (mode === "preview") {
    previewHtml = renderMarkdown(resolveMarkdownImages(text, nodeId, $mdImageCache));
  }

  onMount(() => {
    if (!task) return;
    markEditStart(task);
    text = task.markdown;
    initialText = task.markdown;
    title = markdownTitle(task.markdown);
    view = createMarkdownEditor(host, text, {
      placeholder: "输入 Markdown 内容…",
      onSave: () => void saveAndClose(),
      onClose: () => void saveAndClose(),
      onChange: (value) => (text = value),
      onPasteImage: (file, editor) => void pasteImage(file, editor)
    });
    view.focus();
    return () => {
      view?.destroy();
      view = null;
    };
  });

  function currentText(): string {
    return view ? view.state.doc.toString() : text;
  }

  async function saveAndClose(): Promise<void> {
    if (saving) return;
    const markdown = currentText();
    if (markdown === initialText) {
      clearEditBase(taskId);
      onClose();
      return;
    }
    saving = true;
    const ok = await saveTaskMarkdown(taskId, markdown, hasMultipleMarkdownLines(markdown));
    saving = false;
    if (ok) {
      onClose();
    }
  }

  function toggleMode(next: "edit" | "preview"): void {
    mode = next;
    if (next === "edit") {
      void tick().then(() => view?.focus());
    }
  }

  async function insertImageFile(): Promise<void> {
    if (!isTauriRuntime || !nodeId || !view) return;
    try {
      const srcPath = await pickImageFile();
      if (!srcPath) return;
      const filename = await saveMdImage(srcPath, nodeId);
      const url = await mdImageUrl(nodeId, filename);
      primeMdImageCache(nodeId, filename, url);
      insertAtCursor(view, `\n![](${filename})\n`);
    } catch (error) {
      showToast(`图片插入失败：${String(error)}`);
    }
  }

  async function pasteImage(file: File, editor: EditorView): Promise<void> {
    if (!isTauriRuntime || !nodeId) return;
    try {
      const buffer = await file.arrayBuffer();
      const bytes = new Uint8Array(buffer);
      let binary = "";
      for (let i = 0; i < bytes.length; i += 32768) {
        binary += String.fromCharCode(...bytes.subarray(i, i + 32768));
      }
      const dataUrl = `data:${file.type};base64,${btoa(binary)}`;
      const filename = await saveMdImageFromDataUrl(dataUrl, nodeId);
      const url = await mdImageUrl(nodeId, filename);
      primeMdImageCache(nodeId, filename, url);
      insertAtCursor(editor, `\n![](${filename})\n`);
    } catch (error) {
      showToast(`图片粘贴失败：${String(error)}`);
    }
  }

  function handleBackdropPointerDown(event: PointerEvent): void {
    if (event.target === event.currentTarget) {
      void saveAndClose();
    }
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented) return;
    if (event.key === "Escape") {
      event.preventDefault();
      void saveAndClose();
    } else if (event.key === "s" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void saveAndClose();
    }
  }

  function handlePreviewClick(event: MouseEvent): void {
    const target = event.target as HTMLElement | null;
    const link = target?.closest("a[href]");
    if (!(link instanceof HTMLAnchorElement)) return;
    event.preventDefault();
    event.stopPropagation();
    onOpenLink(link.href);
  }
</script>

<svelte:window on:keydown={handleWindowKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="editor-overlay" on:pointerdown={handleBackdropPointerDown} on:contextmenu|preventDefault|stopPropagation>
  <div class="editor-dialog" role="dialog" aria-label="编辑任务" tabindex="-1" on:pointerdown|stopPropagation on:click|stopPropagation>
    <header class="editor-header">
      <div class="editor-mode-switch" role="tablist">
        <button
          type="button"
          role="tab"
          class:active={mode === "edit"}
          aria-selected={mode === "edit"}
          on:click={() => toggleMode("edit")}
        ><PenLine size={15} /> 编辑</button>
        <button
          type="button"
          role="tab"
          class:active={mode === "preview"}
          aria-selected={mode === "preview"}
          on:click={() => toggleMode("preview")}
        ><Eye size={15} /> 预览</button>
      </div>
      <span class="editor-title" {title}>{title}</span>
      <div class="editor-actions">
        {#if isTauriRuntime && nodeId}
          <button class="editor-icon-button" type="button" title="插入图片" on:click={insertImageFile}>
            <ImagePlus size={17} />
          </button>
        {/if}
        <button class="editor-icon-button primary" type="button" title="保存并关闭（Esc）" disabled={saving} on:click={() => void saveAndClose()}>
          {#if saving}<Check size={17} class="spin" />{:else}<Check size={17} />{/if}
        </button>
        <button class="editor-icon-button" type="button" title="关闭" on:click={() => void saveAndClose()}>
          <X size={17} />
        </button>
      </div>
    </header>

    <div class="editor-body">
      <div bind:this={host} class="editor-cm-host" class:hidden-host={mode !== "edit"}></div>
      {#if mode === "preview"}
        <div class="markdown-body markdown-content editor-preview" on:click={handlePreviewClick}>
          {@html previewHtml}
        </div>
      {/if}
    </div>
  </div>
</div>
