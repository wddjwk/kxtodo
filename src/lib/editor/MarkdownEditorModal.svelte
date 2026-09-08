<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { CalendarDays, Check, Eye, ImagePlus, PenLine, Plus, SmilePlus, Tag as TagIcon, X } from "@lucide/svelte";
  import type { EditorView } from "@codemirror/view";
  import { createMarkdownEditor, insertAtCursor } from "./codemirrorSetup";
  import { hasMultipleMarkdownLines, markdownTitle, renderMarkdown } from "../markdown";
  import { mdImageCache, primeMdImageCache, resolveMarkdownImages } from "../images";
  import {
    isTauriRuntime, mdImageUrl, pickImageFile, saveMdImage, saveMdImageFromDataUrl
  } from "../backend";
  import { caps } from "../capabilities";
  import { appState, clearEditBase, fileToDataUrl, markEditStart, showToast, todayIso } from "../stores";
  import {
    addTask, replaceTaskEmojis, replaceTaskTags, saveTaskMarkdown, updateTask,
    type TaskChanges
  } from "../actions";
  import DatePicker from "../DatePicker.svelte";
  import IconPicker from "../IconPicker.svelte";
  import type { Tag, TagColor } from "../types";

  /** 空串 = 新建模式（配合 draftNodeId），此时保存才创建任务，空正文关掉即消失。 */
  export let taskId: string;
  export let draftNodeId = "";
  export let onClose: () => void = () => {};
  export let onOpenLink: (url: string) => void = () => {};

  const TAG_COLORS: Array<{ color: TagColor; label: string }> = [
    { color: "red", label: "红色" },
    { color: "yellow", label: "黄色" },
    { color: "blue", label: "蓝色" },
    { color: "green", label: "绿色" },
    { color: "gray", label: "灰色" }
  ];

  let host: HTMLDivElement;
  let imageFileInput: HTMLInputElement;
  let view: EditorView | null = null;
  let mode: "edit" | "preview" = "edit";
  let text = "";
  let initialText = "";
  let saving = false;
  let closed = false;
  let title = "编辑任务";

  $: draftMode = !taskId;
  $: task = taskId ? $appState.tasks.find((item) => item.id === taskId) : undefined;
  // 只有编辑模式下「任务不见了」才是异常（被别的设备删了）；新建模式本来就没有任务
  $: if (taskId && !task) {
    onClose();
  }
  $: nodeId = draftMode ? draftNodeId : task?.nodeId ?? "";

  // 元数据：编辑态从任务读初值并记住，新建态是纯本地草稿，保存时随 addTask 一起提交
  let dueDate = "";
  let emojis: string[] = [];
  let tags: Tag[] = [];
  let initialDueDate = "";
  let initialEmojis = "";
  let initialTags = "";
  let metaOpen: "" | "date" | "tag" = "";
  let emojiPickerOpen = false;
  let tagDraft = "";
  let tagColor: TagColor = "yellow";

  // 预览惰性渲染：编辑态不做全量 markdown+高亮（长文档逐键全量渲染会卡死主线程）
  let previewHtml = "";
  $: if (mode === "preview") {
    previewHtml = renderMarkdown(resolveMarkdownImages(text, nodeId, $mdImageCache));
  }
  $: dateLabel = dueDate
    ? dueDate === todayIso()
      ? "今天"
      : `${Number.parseInt(dueDate.slice(5, 7), 10)}月${Number.parseInt(dueDate.slice(8, 10), 10)}日`
    : "";

  onMount(() => {
    // 捕获阶段：对话框对 pointerdown/click 做了 stopPropagation，冒泡阶段收不到里面的交互
    window.addEventListener("pointerdown", dismissPopovers, true);
    window.addEventListener("focusin", dismissPopovers, true);
    text = task?.markdown ?? "";
    initialText = text;
    dueDate = task?.dueDate?.slice(0, 10) ?? "";
    initialDueDate = dueDate;
    emojis = task ? task.emojis.map((emoji) => emoji) : [];
    initialEmojis = emojis.join("");
    tags = task ? task.tags.map((tag) => ({ ...tag })) : [];
    initialTags = JSON.stringify(tags);
    title = draftMode ? "新建事项" : markdownTitle(task?.markdown ?? "");
    view = createMarkdownEditor(host, text, {
      placeholder: "输入 Markdown 内容…",
      onSave: () => void saveAndClose(),
      onClose: () => void saveAndClose(),
      onChange: (value) => (text = value),
      onPasteImage: (file, editor) => void pasteImage(file, editor)
    });
    view.focus();
    return () => {
      window.removeEventListener("pointerdown", dismissPopovers, true);
      window.removeEventListener("focusin", dismissPopovers, true);
      view?.destroy();
      view = null;
    };
  });

  /** 点/焦点落到元数据行之外 → 收起浮层（行内的开关交给触发器自己）。 */
  function dismissPopovers(event: Event): void {
    if (!metaOpen) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest(".editor-meta-field")) return;
    metaOpen = "";
  }

  function currentText(): string {
    return view ? view.state.doc.toString() : text;
  }

  /** 元数据变化落到已有任务上（新建模式随 addTask 一起提交，不走这里）。 */
  async function applyMetaChanges(): Promise<void> {
    if (draftMode || !task) return;
    if (dueDate !== initialDueDate) {
      const changes: TaskChanges = {
        dueDate: dueDate || null,
        plannedDate: dueDate || null
      };
      // 与卡片菜单「添加日期」同一套语义：设成今天就顺带进我的一天
      if (dueDate === todayIso()) changes.myDay = true;
      await updateTask(taskId, changes);
      initialDueDate = dueDate;
    }
    if (emojis.join("") !== initialEmojis) {
      await replaceTaskEmojis(taskId, emojis);
      initialEmojis = emojis.join("");
    }
    if (JSON.stringify(tags) !== initialTags) {
      await replaceTaskTags(taskId, tags);
      initialTags = JSON.stringify(tags);
    }
  }

  /** 落盘。返回 false 表示「还不能关」（正文冲突，或新建时创建失败）。 */
  async function persist(): Promise<boolean> {
    const markdown = currentText();
    if (draftMode) {
      // 空草稿不落盘：点了加号又改主意的，不该留下一条空任务
      if (!markdown.trim()) return true;
      const created = await addTask(draftNodeId, {
        markdown,
        dueDate: dueDate || undefined,
        plannedDate: dueDate || undefined,
        myDay: dueDate === todayIso(),
        tags,
        emojis
      });
      return Boolean(created);
    }
    await applyMetaChanges();
    if (markdown === initialText) {
      clearEditBase(taskId);
      return true;
    }
    const ok = await saveTaskMarkdown(taskId, markdown, hasMultipleMarkdownLines(markdown));
    if (ok) initialText = markdown;
    return ok;
  }

  async function saveAndClose(): Promise<void> {
    if (saving || closed) return;
    saving = true;
    let ok = false;
    try {
      ok = await persist();
    } finally {
      saving = false;
    }
    // 冲突：留在编辑器里让用户再决定，不能当作已保存关掉
    if (!ok) return;
    closed = true;
    onClose();
  }

  // 外部路径卸载（移动端硬件返回弹历史栈）时未保存内容不能丢：尽力保存一次，
  // 与桌面 Esc / 点遮罩"保存并关闭"语义一致。这里只落盘，不再回调 onClose（组件正在拆）。
  onDestroy(() => {
    if (closed || saving) return;
    void persist();
  });

  function toggleMode(next: "edit" | "preview"): void {
    mode = next;
    metaOpen = "";
    if (next === "edit") {
      void tick().then(() => view?.focus());
    }
  }

  function toggleMeta(name: typeof metaOpen): void {
    metaOpen = metaOpen === name ? "" : name;
  }

  function pickDate(date: string): void {
    dueDate = date;
    metaOpen = "";
  }

  function addEmoji(emoji: string): void {
    emojis = [...emojis, emoji];
    emojiPickerOpen = false;
  }

  function removeEmoji(index: number): void {
    emojis = emojis.filter((_, i) => i !== index);
  }

  function addTag(): void {
    const text = tagDraft.trim();
    tags = [
      ...tags,
      { id: `tag-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`, color: tagColor, text: text || undefined }
    ];
    tagDraft = "";
  }

  function removeTag(tagId: string): void {
    tags = tags.filter((tag) => tag.id !== tagId);
  }

  function handleTagKeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) return;
    event.stopPropagation();
    if (event.key === "Enter") {
      event.preventDefault();
      addTag();
    }
  }

  async function insertImageReference(filename: string): Promise<void> {
    const url = await mdImageUrl(nodeId, filename);
    primeMdImageCache(nodeId, filename, url);
    if (view) insertAtCursor(view, `\n![](${filename})\n`);
  }

  async function insertImageFile(): Promise<void> {
    if (!isTauriRuntime || !nodeId || !view) return;
    if (!caps.nativeFileDialogs) {
      // 移动端：无原生文件对话框，走隐藏 <input type=file> + dataURL。
      imageFileInput?.click();
      return;
    }
    try {
      const srcPath = await pickImageFile();
      if (!srcPath) return;
      const filename = await saveMdImage(srcPath, nodeId);
      await insertImageReference(filename);
    } catch (error) {
      showToast(`图片插入失败：${String(error)}`);
    }
  }

  async function insertImageFromInput(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) return;
    if (!isTauriRuntime || !nodeId || !view) return;
    try {
      const dataUrl = await fileToDataUrl(target.files[0]);
      const filename = await saveMdImageFromDataUrl(dataUrl, nodeId);
      await insertImageReference(filename);
    } catch (error) {
      showToast(`图片插入失败：${String(error)}`);
    } finally {
      target.value = "";
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
      if (emojiPickerOpen) {
        emojiPickerOpen = false;
        return;
      }
      if (metaOpen) {
        metaOpen = "";
        return;
      }
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
  <div class="editor-dialog" role="dialog" aria-label={draftMode ? "新建事项" : "编辑任务"} tabindex="-1" on:pointerdown|stopPropagation on:click|stopPropagation>
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
          <Check size={17} />
        </button>
        <button class="editor-icon-button" type="button" title="关闭" on:click={() => void saveAndClose()}>
          <X size={17} />
        </button>
      </div>
    </header>

    <!-- 日期 / 表情 / 标签：与日记编辑器同一套元数据行（样式在 editor.css）。
         日期不设默认值——没填就是没填，不会悄悄给今天。 -->
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="editor-meta" on:click|stopPropagation>
      <div class="editor-meta-field" class:open={metaOpen === "date"}>
        <button class="editor-meta-trigger" type="button" class:filled={Boolean(dueDate)} title="添加日期" on:click={() => toggleMeta("date")}>
          <CalendarDays size={15} />{dateLabel || "添加日期"}
        </button>
        {#if metaOpen === "date"}
          <div class="editor-meta-pop">
            <DatePicker value={dueDate} on:select={(event) => pickDate(event.detail)} on:clear={() => pickDate("")} />
          </div>
        {/if}
      </div>

      <div class="editor-meta-field editor-meta-tags">
        {#each emojis as emoji, index (`emoji-${index}`)}
          <span class="task-emoji-badge" title="点击移除">
            {emoji}
            <button class="tag-delete" type="button" aria-label="移除表情" on:click={() => removeEmoji(index)}>
              <X size={10} strokeWidth={3} />
            </button>
          </span>
        {/each}
        {#each tags as tag (tag.id)}
          <span class={`task-tag tag-${tag.color}`}>
            {tag.text || ""}
            <button class="tag-delete" type="button" aria-label="删除标签" on:click={() => removeTag(tag.id)}>
              <X size={10} strokeWidth={3} />
            </button>
          </span>
        {/each}
        <button class="editor-meta-trigger" type="button" title="添加表情" on:click={() => { emojiPickerOpen = true; metaOpen = ""; }}>
          <SmilePlus size={15} />
        </button>
        <div class="editor-meta-field" class:open={metaOpen === "tag"}>
          <button class="editor-meta-trigger editor-tag-add" type="button" title="标签" on:click={() => toggleMeta("tag")}>
            <TagIcon size={14} />{tags.length ? "" : "标签"}
          </button>
          {#if metaOpen === "tag"}
            <div class="editor-meta-pop editor-tag-pop" on:click|stopPropagation>
              <div class="tag-editor-input-row">
                <input
                  type="text"
                  placeholder="输入标签文字…"
                  maxlength="20"
                  bind:value={tagDraft}
                  on:keydown={handleTagKeydown}
                />
                <button class="tag-add-btn" type="button" title="添加标签" on:click|stopPropagation={addTag}>
                  <Plus size={15} />
                </button>
              </div>
              <div class="tag-editor-colors">
                {#each TAG_COLORS as preset (preset.color)}
                  <button
                    class={`color-circle ${preset.color}`}
                    class:selected={tagColor === preset.color}
                    type="button"
                    title={preset.label}
                    on:click|stopPropagation={() => (tagColor = preset.color)}
                  ></button>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      </div>
    </div>

    <div class="editor-body">
      <div bind:this={host} class="editor-cm-host" class:hidden-host={mode !== "edit"}></div>
      {#if mode === "preview"}
        <div class="markdown-body markdown-content editor-preview" on:click={handlePreviewClick}>
          {@html previewHtml}
        </div>
      {/if}
    </div>

    <input bind:this={imageFileInput} class="hidden-file" type="file" accept="image/*" on:change={insertImageFromInput} />
  </div>

  {#if emojiPickerOpen}
    <IconPicker
      mode="emoji"
      selected={emojis[emojis.length - 1] ?? ""}
      onPick={addEmoji}
      onClose={() => (emojiPickerOpen = false)}
    />
  {/if}
</div>
