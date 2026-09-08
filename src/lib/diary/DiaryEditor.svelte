<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { CalendarDays, Check, CloudSun, Eye, ImagePlus, PenLine, Plus, Smile, Tag as TagIcon, X } from "@lucide/svelte";
  import type { EditorView } from "@codemirror/view";
  import { createMarkdownEditor, insertAtCursor } from "../editor/codemirrorSetup";
  import { renderMarkdown } from "../markdown";
  import { markdownWire } from "../markdownControls";
  import { mdImageCache, primeMdImageCache, resolveMarkdownImages } from "../images";
  import { isTauriRuntime, mdImageUrl, pickImageFile, saveMdImage, saveMdImageFromDataUrl } from "../backend";
  import { caps } from "../capabilities";
  import { get } from "svelte/store";
  import { appSettings, diaryEntries, fileToDataUrl, showToast } from "../stores";
  import { diaryAccent, uiScaleValue } from "../styles";
  import { clampPopoverToViewport } from "../popover";
  import { addDiaryEntry, updateDiaryEntry, type DiaryChanges } from "../actions";
  import DatePicker from "../DatePicker.svelte";
  import {
    DIARY_IMAGE_NODE, MOOD_PRESETS, WEATHER_PRESETS,
    fullDayLabel, relativeDayLabel, todayDate
  } from "../diary";
  import type { DiaryEditorTarget, Tag, TagColor } from "../types";
  import { touchOnly } from "../platform";

  export let target: DiaryEditorTarget;
  export let onClose: () => void = () => {};
  export let onOpenLink: (url: string, title?: string) => void = () => {};

  const TAG_COLORS: Array<{ color: TagColor; label: string }> = [
    { color: "red", label: "红色" },
    { color: "yellow", label: "黄色" },
    { color: "blue", label: "蓝色" },
    { color: "green", label: "绿色" },
    { color: "gray", label: "灰色" }
  ];

  const editingId = "id" in target ? target.id : null;
  const existing = editingId ? get(diaryEntries).find((entry) => entry.id === editingId) : undefined;

  let host: HTMLDivElement;
  let imageFileInput: HTMLInputElement;
  let titleInput: HTMLInputElement;
  let metaRowEl: HTMLDivElement;
  let view: EditorView | null = null;
  let mode: "edit" | "preview" = "edit";
  let saving = false;
  let closed = false;

  let date = existing?.date ?? ("date" in target ? target.date : todayDate());
  let title = existing?.title ?? "";
  let text = existing?.markdown ?? "";
  let mood = existing?.mood ?? "";
  let weather = existing?.weather ?? "";
  let tags: Tag[] = existing ? existing.tags.map((tag) => ({ ...tag })) : [];

  const initial = { date, title, text, mood, weather, tags: JSON.stringify(tags) };

  let openPicker: "" | "date" | "mood" | "weather" | "tag" = "";
  let tagDraft = "";
  let tagColor: TagColor = "yellow";
  /** 触屏上被点了一下、露出删除叉的标签（桌面靠 hover，不用它） */
  let revealedTagId = "";

  function toggleTagReveal(tagId: string): void {
    if (!touchOnly) return;
    revealedTagId = revealedTagId === tagId ? "" : tagId;
  }

  // 预览惰性渲染：编辑态不做全量 markdown + 高亮（长文档逐键全量渲染会卡死主线程）
  let previewHtml = "";
  $: if (mode === "preview") {
    previewHtml = renderMarkdown(resolveMarkdownImages(text, DIARY_IMAGE_NODE, $mdImageCache));
  }
  $: today = todayDate();
  $: dateLabel = date === today ? `今天 · ${fullDayLabel(date)}` : relativeDayLabel(date, today);

  onMount(() => {
    // 捕获阶段：对话框对 pointerdown/click 做了 stopPropagation，冒泡阶段收不到里面的交互
    window.addEventListener("pointerdown", dismissPopovers, true);
    window.addEventListener("focusin", dismissPopovers, true);
    view = createMarkdownEditor(host, text, {
      placeholder: "写下今天……",
      onSave: () => void saveAndClose(),
      onClose: () => void saveAndClose(),
      onChange: (value) => (text = value),
      onPasteImage: (file, editor) => void pasteImage(file, editor)
    });
    // 新建时光标落在标题（先想清楚这天要写什么），改已有的直接进正文续写
    if (editingId) {
      view.focus();
    } else {
      void tick().then(() => titleInput?.focus());
    }
    return () => {
      window.removeEventListener("pointerdown", dismissPopovers, true);
      window.removeEventListener("focusin", dismissPopovers, true);
      view?.destroy();
      view = null;
    };
  });

  /** 点/焦点落到当前打开的那个字段之外 → 收起浮层。字段内部（含它自己的浮层）交给字段自己的开关。 */
  function dismissPopovers(event: Event): void {
    const target = event.target as HTMLElement | null;
    if (revealedTagId && !target?.closest(".task-tag, .task-emoji-badge")) {
      revealedTagId = "";
    }
    if (!openPicker) return;
    if (target?.closest(".editor-meta-field")) return;
    openPicker = "";
  }

  // 外部路径卸载（移动端硬件返回弹历史栈）时未保存内容不能丢：尽力保存一次，
  // 与桌面 Esc / 点遮罩「保存并关闭」语义一致。
  onDestroy(() => {
    if (closed || saving) return;
    void persist();
  });

  function currentText(): string {
    return view ? view.state.doc.toString() : text;
  }

  /** 落盘（新建或修改）。标题与正文都空时新建不落盘——空草稿关掉就该消失。 */
  async function persist(): Promise<void> {
    const markdown = currentText();
    const trimmedTitle = title.trim();
    if (!trimmedTitle && !markdown.trim()) {
      if (!editingId) return;
      showToast("标题与正文不能同时为空");
      return;
    }
    if (editingId) {
      const changes: DiaryChanges = {};
      if (date !== initial.date) changes.date = date;
      if (trimmedTitle !== initial.title) changes.title = trimmedTitle;
      if (markdown !== initial.text) changes.markdown = markdown;
      if (mood !== initial.mood) changes.mood = mood;
      if (weather !== initial.weather) changes.weather = weather;
      if (JSON.stringify(tags) !== initial.tags) changes.tags = tags;
      if (Object.keys(changes).length === 0) return;
      await updateDiaryEntry(editingId, changes);
      return;
    }
    await addDiaryEntry({ date, title: trimmedTitle, markdown, mood, weather, tags });
  }

  async function saveAndClose(): Promise<void> {
    if (saving || closed) return;
    saving = true;
    try {
      await persist();
    } finally {
      saving = false;
    }
    closed = true;
    onClose();
  }

  function toggleMode(next: "edit" | "preview"): void {
    mode = next;
    openPicker = "";
    if (next === "edit") {
      void tick().then(() => view?.focus());
    }
  }

  function togglePicker(name: typeof openPicker): void {
    openPicker = openPicker === name ? "" : name;
    if (openPicker) {
      void clampPopoverToViewport(metaRowEl, uiScaleValue($appSettings.appearance.uiScale));
    }
  }

  function pickDate(value: string): void {
    date = value;
    openPicker = "";
  }

  function pickMood(emoji: string): void {
    mood = mood === emoji ? "" : emoji;
    openPicker = "";
  }

  function pickWeather(emoji: string): void {
    weather = weather === emoji ? "" : emoji;
    openPicker = "";
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
    const url = await mdImageUrl(DIARY_IMAGE_NODE, filename);
    primeMdImageCache(DIARY_IMAGE_NODE, filename, url);
    if (view) insertAtCursor(view, `\n![](${filename})\n`);
  }

  async function insertImageFile(): Promise<void> {
    if (!isTauriRuntime || !view) return;
    if (!caps.nativeFileDialogs) {
      // 移动端：无原生文件对话框，走隐藏 <input type=file> + dataURL
      imageFileInput?.click();
      return;
    }
    try {
      const srcPath = await pickImageFile();
      if (!srcPath) return;
      await insertImageReference(await saveMdImage(srcPath, DIARY_IMAGE_NODE));
    } catch (error) {
      showToast(`图片插入失败：${String(error)}`);
    }
  }

  async function insertImageFromInput(event: Event): Promise<void> {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement) || !input.files?.[0]) return;
    if (!isTauriRuntime || !view) return;
    try {
      const dataUrl = await fileToDataUrl(input.files[0]);
      await insertImageReference(await saveMdImageFromDataUrl(dataUrl, DIARY_IMAGE_NODE));
    } catch (error) {
      showToast(`图片插入失败：${String(error)}`);
    } finally {
      input.value = "";
    }
  }

  async function pasteImage(file: File, editor: EditorView): Promise<void> {
    if (!isTauriRuntime) return;
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      let binary = "";
      for (let index = 0; index < bytes.length; index += 32768) {
        binary += String.fromCharCode(...bytes.subarray(index, index + 32768));
      }
      const dataUrl = `data:${file.type};base64,${btoa(binary)}`;
      const filename = await saveMdImageFromDataUrl(dataUrl, DIARY_IMAGE_NODE);
      const url = await mdImageUrl(DIARY_IMAGE_NODE, filename);
      primeMdImageCache(DIARY_IMAGE_NODE, filename, url);
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
      if (openPicker) {
        openPicker = "";
        return;
      }
      void saveAndClose();
    } else if (event.key === "s" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void saveAndClose();
    }
  }

  function handlePreviewClick(event: MouseEvent): void {
    const link = (event.target as HTMLElement | null)?.closest("a[href]");
    if (!(link instanceof HTMLAnchorElement)) return;
    event.preventDefault();
    event.stopPropagation();
    onOpenLink(link.href, (link.textContent ?? "").trim());
  }
</script>

<svelte:window on:keydown={handleWindowKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="editor-overlay" on:pointerdown={handleBackdropPointerDown} on:contextmenu|preventDefault|stopPropagation>
  <!-- --accent 内联：编辑器浮层挂在 App 层，拿不到 .diary-view 的主题色，跟着用户选的日记色走 -->
  <div class="editor-dialog diary-editor" style={`--accent: ${diaryAccent($appSettings.diary)}`} role="dialog" aria-label="编辑日记" tabindex="-1" on:pointerdown|stopPropagation on:click|stopPropagation>
    <header class="editor-header">
      <div class="editor-mode-switch" role="tablist">
        <button type="button" role="tab" class:active={mode === "edit"} aria-selected={mode === "edit"} on:click={() => toggleMode("edit")}>
          <PenLine size={15} />编辑
        </button>
        <button type="button" role="tab" class:active={mode === "preview"} aria-selected={mode === "preview"} on:click={() => toggleMode("preview")}>
          <Eye size={15} />预览
        </button>
      </div>
      <span class="editor-title">{editingId ? "编辑日记" : "写日记"}</span>
      <div class="editor-actions">
        {#if isTauriRuntime}
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

    <!-- 日期 / 心情 / 天气 / 标签：一行元数据，浮层都在对话框内向下展开 -->
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="editor-meta" bind:this={metaRowEl} on:click|stopPropagation>
      <div class="editor-meta-field" class:open={openPicker === "date"}>
        <button class="editor-meta-trigger" type="button" title="归属日期" on:click={() => togglePicker("date")}>
          <CalendarDays size={15} />{dateLabel}
        </button>
        {#if openPicker === "date"}
          <div class="editor-meta-pop">
            <DatePicker value={date} on:select={(event) => pickDate(event.detail)} on:clear={() => pickDate(today)} />
          </div>
        {/if}
      </div>

      <div class="editor-meta-field" class:open={openPicker === "mood"}>
        <button class="editor-meta-trigger" type="button" class:filled={Boolean(mood)} title="心情" on:click={() => togglePicker("mood")}>
          <Smile size={15} />{mood || "心情"}
        </button>
        {#if openPicker === "mood"}
          <div class="editor-meta-pop editor-emoji-grid-pop">
            {#each MOOD_PRESETS as preset (preset.emoji)}
              <button class="emoji-pick-cell" type="button" class:selected={mood === preset.emoji} title={preset.label} on:click={() => pickMood(preset.emoji)}>
                {preset.emoji}
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <div class="editor-meta-field" class:open={openPicker === "weather"}>
        <button class="editor-meta-trigger" type="button" class:filled={Boolean(weather)} title="天气" on:click={() => togglePicker("weather")}>
          <CloudSun size={15} />{weather || "天气"}
        </button>
        {#if openPicker === "weather"}
          <div class="editor-meta-pop editor-emoji-grid-pop">
            {#each WEATHER_PRESETS as preset (preset.emoji)}
              <button class="emoji-pick-cell" type="button" class:selected={weather === preset.emoji} title={preset.label} on:click={() => pickWeather(preset.emoji)}>
                {preset.emoji}
              </button>
            {/each}
          </div>
        {/if}
      </div>

      <div class="editor-meta-field editor-meta-tags" class:open={openPicker === "tag"}>
        {#each tags as tag (tag.id)}
          <span
            class={`task-tag tag-${tag.color}`}
            class:reveal-delete={revealedTagId === tag.id}
            on:click|stopPropagation={() => toggleTagReveal(tag.id)}
          >
            {tag.text || ""}
            <button class="tag-delete" type="button" aria-label="删除标签" on:click|stopPropagation={() => removeTag(tag.id)}>
              <X size={10} strokeWidth={3} />
            </button>
          </span>
        {/each}
        <button class="editor-meta-trigger editor-tag-add" type="button" title="标签" on:click={() => togglePicker("tag")}>
          <TagIcon size={14} />{tags.length ? "" : "标签"}
        </button>
        {#if openPicker === "tag"}
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

    <input
      bind:this={titleInput}
      bind:value={title}
      class="editor-title-input"
      type="text"
      maxlength="120"
      placeholder="标题（可留空，直接写正文）"
      on:keydown={(event) => { if (event.key === "Enter" && !event.isComposing && event.keyCode !== 229) { event.preventDefault(); view?.focus(); } }}
    />

    <div class="editor-body">
      <div bind:this={host} class="editor-cm-host" class:hidden-host={mode !== "edit"}></div>
      {#if mode === "preview"}
        <div class="markdown-body markdown-content editor-preview" use:markdownWire on:click={handlePreviewClick}>
          {@html previewHtml}
        </div>
      {/if}
    </div>

    <input bind:this={imageFileInput} class="hidden-file" type="file" accept="image/*" on:change={insertImageFromInput} />
  </div>
</div>
