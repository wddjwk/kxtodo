<script lang="ts">
  /**
   * 日程编辑器（v0.9.0）：标题 / 全天开关 / 起止「日期+时刻」/ 提醒 / Markdown 详情。
   *
   * 与日记编辑器同一套骨架（editor-overlay + editor-dialog + createMarkdownEditor +
   * Esc/遮罩 = 保存并关闭 + 移动端历史栈一层）。**关联条目的详情是双向的**：
   * 这里保存详情 → `updateCalendarEvent` 连带写任务 markdown；任务那边改内容 →
   * `syncTaskMarkdownToLinkedEvents` 写回日程详情（等值判断收敛，不打乒乓）。
   *
   * 新建时**保存才落盘**：拖选时间段后改主意直接关掉，不留空日程；
   * 标题与详情全空的「保存并关闭」视为放弃（显式点保存按钮则总是创建——
   * 只占时间块不写名字的日程是合法的）。
   */
  import { onDestroy, onMount, tick } from "svelte";
  import { get } from "svelte/store";
  import {
    AlarmClock, Bell, CalendarDays, Check, Eye, ImagePlus, Link2Off, PenLine,
    Trash2, X
  } from "@lucide/svelte";
  import type { EditorView } from "@codemirror/view";
  import { createMarkdownEditor, insertAtCursor, replaceDocument } from "../editor/codemirrorSetup";
  import { renderMarkdown } from "../markdown";
  import { markdownWire } from "../markdownControls";
  import { taskToggleIndex, toggleMarkdownTask } from "../markdownTasks";
  import { mdImageCache, primeMdImageCache, resolveMarkdownImages } from "../images";
  import { isTauriRuntime, mdImageUrl, pickImageFile, saveMdImage, saveMdImageFromDataUrl } from "../backend";
  import { caps } from "../capabilities";
  import { appSettings, appState, calendarEvents, fileToDataUrl, showToast } from "../stores";
  import { calendarAccent, uiScaleValue } from "../styles";
  import { clampPopoverToViewport } from "../popover";
  import { addCalendarEvent, deleteCalendarEvent, updateCalendarEvent, type CalendarChanges } from "../actions";
  import { suppressGhostClick } from "../ghostClick";
  import DatePicker from "../DatePicker.svelte";
  import MarkdownToolbar from "../editor/MarkdownToolbar.svelte";
  import { CALENDAR_IMAGE_NODE, durationText, reminderLabel } from "../calendar";
  import { calendarToday } from "../calendar";
  import { fullDayLabel, relativeDayLabel } from "../diary";
  import { nowClock } from "../clock";
  import type { CalendarEditorTarget, ReminderRule } from "../types";
  import { createBackGuard, isMobile, touchOnly } from "../platform";
  import { imeInset } from "../imeInset";
  void touchOnly;

  export let target: CalendarEditorTarget;
  export let onClose: () => void = () => {};
  export let onOpenLink: (url: string, title?: string) => void = () => {};

  const editingId = "id" in target ? target.id : null;
  const existing = editingId ? get(calendarEvents).find((event) => event.id === editingId) : undefined;
  const linkedTaskId0 = existing?.taskId || ("taskId" in target ? target.taskId ?? "" : "");
  const linkedTask0 = linkedTaskId0
    ? get(appState).tasks.find((task) => task.id === linkedTaskId0)
    : undefined;

  function taskSummary(markdown: string): string {
    const first = markdown
      .split("\n")
      .map((line) => line.trim())
      .find((line) => line.length > 0);
    return (first ?? "").replace(/^[-*]\s*(\[[ xX]\]\s*)?/, "").slice(0, 40) || "(无标题条目)";
  }

  function nextFullHour(): string {
    const now = new Date();
    const hour = (now.getHours() + 1) % 24;
    return `${`${hour}`.padStart(2, "0")}:00`;
  }

  function defaultStart(): { date: string; time: string; allDay: boolean } {
    if ("id" in target) {
      return { date: existing?.startDate ?? calendarToday(), time: existing?.startTime ?? "", allDay: existing?.allDay ?? false };
    }
    return {
      date: target.startDate,
      time: target.allDay ? "" : target.startTime ?? nextFullHour(),
      allDay: target.allDay === true || (!target.startTime && false)
    };
  }

  const start0 = defaultStart();

  let host: HTMLDivElement;
  let imageFileInput: HTMLInputElement;
  let titleInput: HTMLInputElement;
  let metaRowEl: HTMLDivElement;
  let view: EditorView | null = null;
  let mode: "edit" | "preview" = "edit";
  let saving = false;
  let closed = false;

  let title = existing?.title ?? (linkedTask0 ? taskSummary(linkedTask0.markdown) : "");
  let allDay = start0.allDay;
  let startDate = start0.date;
  let startTime = allDay ? "" : start0.time || nextFullHour();
  let endDate = existing?.endDate ?? ("endDate" in target ? target.endDate ?? startDate : startDate);
  let endTime = existing?.endTime ?? ("endTime" in target ? target.endTime ?? "" : "");
  if (!existing && !("endDate" in target && target.endDate)) {
    // 新建且没预置结束：默认一小时
    const total = allDay ? 0 : Number(startTime.slice(0, 2)) * 60 + Number(startTime.slice(3, 5)) + 60;
    if (!allDay) {
      endDate = total >= 1440 ? addDaysLocal(startDate, 1) : startDate;
      endTime = `${`${Math.floor((total % 1440) / 60)}`.padStart(2, "0")}:${`${(total % 1440) % 60}`.padStart(2, "0")}`;
    }
  }
  let reminders: ReminderRule[] = existing ? existing.reminders.map((rule) => ({ ...rule })) : [];
  let taskId = linkedTaskId0;
  let text = existing ? existing.details : (linkedTask0 ? linkedTask0.markdown : "");

  function addDaysLocal(date: string, days: number): string {
    const [y, m, d] = date.split("-").map(Number);
    const dt = new Date(Date.UTC(y, m - 1, d + days));
    return `${dt.getUTCFullYear()}-${`${dt.getUTCMonth() + 1}`.padStart(2, "0")}-${`${dt.getUTCDate()}`.padStart(2, "0")}`;
  }

  const initial = {
    title,
    allDay,
    startDate,
    startTime,
    endDate,
    endTime,
    text,
    reminders: JSON.stringify(reminders),
    taskId
  };

  let openPicker: "" | "start" | "end" | "reminder" = "";
  /** 自定义绝对提醒的日期与时刻草稿 */
  let customDate = calendarToday();
  let customTime = nowClock();

  const REMINDER_PRESETS: Array<{ label: string; minutes: number }> = [
    { label: "日程开始时", minutes: 0 },
    { label: "提前 5 分钟", minutes: 5 },
    { label: "提前 15 分钟", minutes: 15 },
    { label: "提前 30 分钟", minutes: 30 },
    { label: "提前 1 小时", minutes: 60 },
    { label: "提前 1 天", minutes: 1440 }
  ];

  // 预览惰性渲染：编辑态不做全量 markdown + 高亮
  let previewHtml = "";
  $: if (mode === "preview") {
    previewHtml = renderMarkdown(resolveMarkdownImages(text, CALENDAR_IMAGE_NODE, $mdImageCache));
  }
  $: today = calendarToday();
  $: startLabel = startDate === today
    ? `今天 · ${fullDayLabel(startDate)}`
    : `${relativeDayLabel(startDate, today)} · ${fullDayLabel(startDate)}`;
  $: endLabel = endDate === today
    ? `今天 · ${fullDayLabel(endDate)}`
    : `${relativeDayLabel(endDate, today)} · ${fullDayLabel(endDate)}`;
  $: duration = durationText({
    startDate,
    startTime: allDay ? "" : startTime,
    endDate,
    endTime: allDay ? "" : endTime,
    allDay
  });
  $: linkedTask = taskId ? $appState.tasks.find((task) => task.id === taskId) : undefined;
  // 桌面编辑器尺寸按窗口比例（设置项）；移动端固定铺满不给内联值
  $: editorSizeStyle = $isMobile
    ? ""
    : `width: ${$appSettings.appearance.editorWidthPercent}%; height: ${$appSettings.appearance.editorHeightPercent}%;`;

  onMount(() => {
    window.addEventListener("pointerdown", dismissPopovers, true);
    window.addEventListener("focusin", dismissPopovers, true);
    view = createMarkdownEditor(host, text, {
      placeholder: taskId ? "详细信息与关联条目内容双向同步……" : "详细信息（Markdown）……",
      onSave: () => void saveAndClose(true),
      onClose: () => void saveAndClose(true),
      onChange: (value) => (text = value),
      onPasteImage: (file, editor) => void pasteImage(file, editor)
    });
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

  function dismissPopovers(event: Event): void {
    if (!openPicker) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest(".editor-meta-field")) return;
    openPicker = "";
  }

  // 外部路径卸载（移动端硬件返回弹历史栈）时未保存内容不能丢：尽力保存一次
  onDestroy(() => {
    if (closed || saving) return;
    void persist(true);
  });

  function currentText(): string {
    return view ? view.state.doc.toString() : text;
  }

  /** 起止校验：定时事件结束不得早于开始（全天按日期比较）。返回错误文案或空串。 */
  function validateRange(): string {
    if (endDate < startDate) return "结束日期不能早于开始日期";
    if (!allDay && endDate === startDate && endTime !== "" && endTime < startTime) {
      return "结束时间不能早于开始时间";
    }
    return "";
  }

  /** 落盘（新建或修改）。`implicit` = Esc/遮罩/返回键触发的保存：全空的新建直接放弃。 */
  async function persist(implicit: boolean): Promise<void> {
    const details = currentText();
    const trimmedTitle = title.trim();
    if (!editingId && implicit && !trimmedTitle && !details.trim()) return;
    const rangeError = validateRange();
    if (rangeError) {
      showToast(rangeError);
      return;
    }
    if (editingId) {
      const changes: CalendarChanges = {};
      if (trimmedTitle !== initial.title) changes.title = trimmedTitle;
      if (details !== initial.text) changes.details = details;
      if (allDay !== initial.allDay) changes.allDay = allDay;
      if (startDate !== initial.startDate) changes.startDate = startDate;
      const nextStart = allDay ? "" : startTime;
      const nextEnd = allDay ? "" : endTime;
      if (nextStart !== initial.startTime) changes.startTime = nextStart;
      if (endDate !== initial.endDate) changes.endDate = endDate;
      if (nextEnd !== initial.endTime) changes.endTime = nextEnd;
      if (JSON.stringify(reminders) !== initial.reminders) changes.reminders = reminders;
      if (taskId !== initial.taskId) changes.taskId = taskId;
      if (Object.keys(changes).length === 0) return;
      await updateCalendarEvent(editingId, changes);
      return;
    }
    await addCalendarEvent({
      title: trimmedTitle,
      details,
      startDate,
      startTime: allDay ? "" : startTime,
      endDate,
      endTime: allDay ? "" : endTime,
      allDay,
      reminders,
      taskId
    });
  }

  async function saveAndClose(implicit = false): Promise<void> {
    if (saving || closed) return;
    saving = true;
    try {
      await persist(implicit);
    } finally {
      saving = false;
    }
    closed = true;
    onClose();
  }

  async function removeAndClose(): Promise<void> {
    if (!editingId || saving || closed) return;
    saving = true;
    try {
      await deleteCalendarEvent(editingId);
      showToast("日程已删除");
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

  // 编辑器里的日期 / 提醒浮层：返回键先收掉它们，再按一次才轮到历史栈弹编辑器
  const backGuard = createBackGuard();
  $: backGuard(openPicker !== "", () => (openPicker = ""));

  function togglePicker(name: typeof openPicker): void {
    openPicker = openPicker === name ? "" : name;
    if (openPicker) {
      void clampPopoverToViewport(metaRowEl, uiScaleValue($appSettings.appearance.uiScale));
    }
  }

  /** 全天开关：关掉时给一个合理的默认时刻（下一个整点起一小时） */
  function toggleAllDay(): void {
    allDay = !allDay;
    if (allDay) {
      endTime = "";
      return;
    }
    if (!startTime) startTime = nextFullHour();
    if (!endTime) {
      const total = Number(startTime.slice(0, 2)) * 60 + Number(startTime.slice(3, 5)) + 60;
      endDate = total >= 1440 ? addDaysLocal(startDate, 1) : startDate;
      endTime = `${`${Math.floor((total % 1440) / 60)}`.padStart(2, "0")}:${`${(total % 1440) % 60}`.padStart(2, "0")}`;
    }
  }

  /** 改了开始：结束早于开始就跟着平移（保持时长，业界编辑器同款行为） */
  function bumpEndAfterStartChange(): void {
    if (allDay) {
      if (endDate < startDate) endDate = startDate;
      return;
    }
    const startAbs = minutesFromClock(startTime);
    const endAbs = diffDaysLocal(startDate, endDate) * 1440 + minutesFromClock(endTime || startTime);
    if (endAbs <= startAbs) {
      const total = startAbs + 60;
      endDate = addDaysLocal(startDate, Math.floor(total / 1440));
      endTime = `${`${Math.floor((total % 1440) / 60)}`.padStart(2, "0")}:${`${(total % 1440) % 60}`.padStart(2, "0")}`;
    }
  }

  function minutesFromClock(clock: string): number {
    return Number(clock.slice(0, 2)) * 60 + Number(clock.slice(3, 5));
  }

  function diffDaysLocal(from: string, to: string): number {
    const [fy, fm, fd] = from.split("-").map(Number);
    const [ty, tm, td] = to.split("-").map(Number);
    return Math.round((Date.UTC(ty, tm - 1, td) - Date.UTC(fy, fm - 1, fd)) / 86_400_000);
  }

  function pickStartDate(date: string): void {
    startDate = date;
    bumpEndAfterStartChange();
  }

  function pickStartTime(time: string): void {
    startTime = time;
    bumpEndAfterStartChange();
  }

  function pickEndDate(date: string): void {
    endDate = date;
  }

  function pickEndTime(time: string): void {
    endTime = time;
  }

  // ---- 提醒 ----

  function addBeforeReminder(minutes: number): void {
    if (reminders.some((rule) => rule.kind === "beforeDue" && rule.minutes === minutes)) return;
    reminders = [...reminders, { kind: "beforeDue", minutes }];
  }

  function addCustomReminder(): void {
    const at = new Date(`${customDate}T${customTime}:00`);
    if (Number.isNaN(at.getTime())) {
      showToast("自定义提醒时刻无效");
      return;
    }
    const iso = at.toISOString();
    if (reminders.some((rule) => rule.kind === "absolute" && rule.at === iso)) return;
    reminders = [...reminders, { kind: "absolute", at: iso }];
    openPicker = "";
  }

  function removeReminder(index: number): void {
    reminders = reminders.filter((_, position) => position !== index);
  }

  function unlinkTask(): void {
    taskId = "";
  }

  // ---- 详情插图（走 calendar 伪条目通道，与日记同一套） ----

  async function insertImageReference(filename: string): Promise<void> {
    const url = await mdImageUrl(CALENDAR_IMAGE_NODE, filename);
    primeMdImageCache(CALENDAR_IMAGE_NODE, filename, url);
    if (view) insertAtCursor(view, `\n![](${filename})\n`);
  }

  async function insertImageFile(): Promise<void> {
    if (!isTauriRuntime || !view) return;
    if (!caps.nativeFileDialogs) {
      imageFileInput?.click();
      return;
    }
    try {
      const srcPath = await pickImageFile();
      if (!srcPath) return;
      await insertImageReference(await saveMdImage(srcPath, CALENDAR_IMAGE_NODE));
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
      await insertImageReference(await saveMdImageFromDataUrl(dataUrl, CALENDAR_IMAGE_NODE));
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
      const filename = await saveMdImageFromDataUrl(dataUrl, CALENDAR_IMAGE_NODE);
      const url = await mdImageUrl(CALENDAR_IMAGE_NODE, filename);
      primeMdImageCache(CALENDAR_IMAGE_NODE, filename, url);
      insertAtCursor(editor, `\n![](${filename})\n`);
    } catch (error) {
      showToast(`图片粘贴失败：${String(error)}`);
    }
  }

  /** 桌面右击仍不弹浏览器原生菜单；移动端必须放行长按菜单（复制/剪切/全选/粘贴）。 */
  function handleOverlayContext(event: MouseEvent): void {
    event.stopPropagation();
    if (!$isMobile) event.preventDefault();
  }

  function handleBackdropPointerDown(event: PointerEvent): void {
    if (event.target !== event.currentTarget) return;
    event.preventDefault();
    suppressGhostClick({ x: event.clientX, y: event.clientY });
    void saveAndClose(true);
  }

  function handleWindowKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented) return;
    if (event.key === "Escape") {
      event.preventDefault();
      if (openPicker) {
        openPicker = "";
        return;
      }
      void saveAndClose(true);
    } else if (event.key === "s" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void saveAndClose(false);
    }
  }

  function handlePreviewClick(event: MouseEvent): void {
    const boxIndex = taskToggleIndex(event, event.currentTarget as Element);
    if (boxIndex !== null) {
      event.preventDefault();
      event.stopPropagation();
      const next = toggleMarkdownTask(text, boxIndex);
      if (view) replaceDocument(view, next);
      else text = next;
      return;
    }
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
<div class="editor-overlay" use:imeInset on:pointerdown={handleBackdropPointerDown} on:contextmenu={handleOverlayContext}>
  <!-- --accent 内联：编辑器浮层挂在 App 层，拿不到 .calendar-view 的主题色，跟着用户选的日程色走 -->
  <div class="editor-dialog calendar-editor" style={`--accent: ${calendarAccent($appSettings.calendar)}; ${editorSizeStyle}`} role="dialog" aria-label={editingId ? "编辑日程" : "新建日程"} tabindex="-1" on:pointerdown|stopPropagation on:click|stopPropagation>
    <header class="editor-header">
      <div class="editor-mode-switch" role="tablist" data-mode={mode}>
        <span class="editor-mode-thumb" aria-hidden="true"></span>
        <button type="button" role="tab" class:active={mode === "edit"} aria-selected={mode === "edit"} on:click={() => toggleMode("edit")}>
          <PenLine size={15} />编辑
        </button>
        <button type="button" role="tab" class:active={mode === "preview"} aria-selected={mode === "preview"} on:click={() => toggleMode("preview")}>
          <Eye size={15} />预览
        </button>
      </div>
      <span class="editor-title">{editingId ? "编辑日程" : "新建日程"}</span>
      <div class="editor-actions">
        {#if editingId}
          <button class="editor-icon-button danger" type="button" title="删除日程" disabled={saving} on:click={() => void removeAndClose()}>
            <Trash2 size={17} />
          </button>
        {/if}
        {#if isTauriRuntime}
          <button class="editor-icon-button" type="button" title="插入图片" on:click={insertImageFile}>
            <ImagePlus size={17} />
          </button>
        {/if}
        <button class="editor-icon-button primary" type="button" title="保存并关闭（Esc）" disabled={saving} on:click={() => void saveAndClose(false)}>
          <Check size={17} />
        </button>
        <button class="editor-icon-button" type="button" title="关闭" on:click={() => void saveAndClose(true)}>
          <X size={17} />
        </button>
      </div>
    </header>

    <input
      bind:this={titleInput}
      bind:value={title}
      class="editor-title-input calendar-title-input"
      type="text"
      maxlength="200"
      placeholder="日程名称"
      on:keydown={(event) => { if (event.key === "Enter" && !event.isComposing && event.keyCode !== 229) { event.preventDefault(); view?.focus(); } }}
    />

    <!-- 时间 / 全天 / 提醒 / 关联条目：一行元数据，浮层都在对话框内向下展开 -->
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="editor-meta calendar-meta" bind:this={metaRowEl} on:click|stopPropagation>
      <label class="calendar-allday" title="全天日程不占时间网格，显示在日/周视图顶部的全天栏">
        <input type="checkbox" checked={allDay} on:change={toggleAllDay} />
        <span>全天</span>
      </label>

      <div class="editor-meta-field" class:open={openPicker === "start"}>
        <button class="editor-meta-trigger" type="button" title="开始时间" on:click={() => togglePicker("start")}>
          <CalendarDays size={15} />{startLabel}{#if !allDay}<span class="calendar-meta-time">{startTime || "--:--"}</span>{/if}
        </button>
        {#if openPicker === "start"}
          <div class="editor-meta-pop">
            <DatePicker
              value={startDate}
              time={allDay ? "" : startTime}
              withTime={!allDay}
              on:select={(event) => pickStartDate(event.detail)}
              on:selectTime={(event) => pickStartTime(event.detail)}
              on:clear={() => pickStartDate(today)}
              on:close={() => (openPicker = "")}
            />
          </div>
        {/if}
      </div>

      <div class="editor-meta-field" class:open={openPicker === "end"}>
        <button class="editor-meta-trigger" type="button" title="结束时间" on:click={() => togglePicker("end")}>
          <CalendarDays size={15} />{endLabel}{#if !allDay}<span class="calendar-meta-time">{endTime || "--:--"}</span>{/if}
        </button>
        {#if openPicker === "end"}
          <div class="editor-meta-pop">
            <DatePicker
              value={endDate}
              time={allDay ? "" : endTime}
              withTime={!allDay}
              on:select={(event) => pickEndDate(event.detail)}
              on:selectTime={(event) => pickEndTime(event.detail)}
              on:clear={() => pickEndDate(startDate)}
              on:close={() => (openPicker = "")}
            />
          </div>
        {/if}
      </div>

      {#if duration}
        <span class="calendar-duration" title="时长">{duration}</span>
      {/if}

      <div class="editor-meta-field calendar-reminders" class:open={openPicker === "reminder"}>
        {#each reminders as rule, index (`${rule.kind}-${index}`)}
          <span class="task-tag tag-blue calendar-reminder-chip" title={reminderLabel(rule)}>
            <AlarmClock size={11} />
            {reminderLabel(rule)}
            <button class="tag-delete" type="button" aria-label="移除提醒" on:click|stopPropagation={() => removeReminder(index)}>
              <X size={10} strokeWidth={3} />
            </button>
          </span>
        {/each}
        <button class="editor-meta-trigger" type="button" title="添加提醒" on:click={() => togglePicker("reminder")}>
          <Bell size={14} />{#if !reminders.length}<span class="trigger-label">提醒</span>{/if}
        </button>
        {#if openPicker === "reminder"}
          <div class="editor-meta-pop calendar-reminder-pop" on:click|stopPropagation>
            {#each REMINDER_PRESETS as preset (preset.minutes)}
              <button
                class="menu-action-button calendar-reminder-preset"
                type="button"
                on:click|stopPropagation={() => addBeforeReminder(preset.minutes)}
              >{preset.label}</button>
            {/each}
            <div class="calendar-reminder-custom">
              <span>自定义时刻</span>
              <input type="date" bind:value={customDate} on:keydown|stopPropagation />
              <input type="time" bind:value={customTime} on:keydown|stopPropagation />
              <button class="menu-action-button" type="button" on:click|stopPropagation={addCustomReminder}>添加</button>
            </div>
          </div>
        {/if}
      </div>

      {#if taskId}
        <span class="task-tag tag-purple calendar-linked-chip" title="详细信息与这个条目的内容双向同步">
          <PenLine size={11} />
          {linkedTask ? taskSummary(linkedTask.markdown) : "关联条目"}
          <button class="tag-delete" type="button" aria-label="解除关联" on:click|stopPropagation={unlinkTask}>
            <Link2Off size={10} strokeWidth={3} />
          </button>
        </span>
      {/if}
    </div>

    {#if mode === "edit" && $appSettings.features.editorToolbar}
      <MarkdownToolbar view={view} onImage={() => void insertImageFile()} />
    {/if}

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
