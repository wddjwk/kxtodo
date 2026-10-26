<script lang="ts">
  /**
   * 日程编辑器（v0.9.0；**v0.9.1 重做成设置式表单**）。
   *
   * 需求原话：「我要的不是一个编辑器界面，而是一个正经的日程配置界面……开始、结束时间，
   * 提醒、地点、分类等，都是排列整齐对仗工整的有设计感的项目，而不是混杂在编辑器的
   * 工具栏。编辑器应该是嵌在创建日程的界面的一个区块而已。」
   *
   * 于是这一版的骨架是**表单**：名称 + 一列「标签 / 控件」两栏对齐的行（全天、开始、
   * 结束、时长、重复、提醒、地点、分类、关联条目），Markdown 详情只是最后一个区块。
   * 行里点下去的浮层一律交给 `ContextMenu` 宿主——它自带壳内定位、四边钳制、
   * Esc/点外关闭、输入法不抢焦点与移动端钻入，v0.9.0 那套「浮层塞在 overflow:hidden
   * 的对话框里被裁掉」的问题一并没了（需求 5）。
   *
   * 移动端**不自动进编辑态**（需求 4 后半）：进去只渲染详情的**只读渲染**（空则一行
   * 提示），点一下详情区块才挂 CodeMirror 并聚焦——标题框也不自动聚焦，免得一进页面
   * 就弹出输入法。桌面照旧：新建聚焦标题、编辑聚焦详情。
   *
   * 保留 v0.9.0 的写路径纪律：新建时**保存才落盘**（拖选时间段后改主意直接关掉不留空
   * 日程）；标题与详情全空的「保存并关闭」视为放弃；关联条目的详情**双向同步**。
   */
  import { onDestroy, onMount, tick } from "svelte";
  import { get } from "svelte/store";
  import {
    AlarmClock, Bell, CalendarDays, Check, ChevronRight, Eye, ImagePlus, Link2Off,
    MapPin, PenLine, Repeat as RepeatIcon, SlidersHorizontal, Tags, Trash2, X
  } from "@lucide/svelte";
  import type { EditorView } from "@codemirror/view";
  import { createMarkdownEditor, insertAtCursor, replaceDocument } from "../editor/codemirrorSetup";
  import { renderMarkdown } from "../markdown";
  import { markdownWire } from "../markdownControls";
  import { taskToggleIndex, toggleMarkdownTask } from "../markdownTasks";
  import { mdImageCache, primeMdImageCache, resolveMarkdownImages } from "../images";
  import { isTauriRuntime, mdImageUrl, pickImageFile, saveMdImage, saveMdImageFromDataUrl } from "../backend";
  import { caps } from "../capabilities";
  import { appSettings, appState, calendarCategories, calendarEvents, fileToDataUrl, showToast } from "../stores";
  import { calendarAccent } from "../styles";
  import {
    addCalendarEvent, defaultCalendarCategoryId, deleteCalendarEvent, updateCalendarEvent,
    type CalendarChanges
  } from "../actions";
  import { suppressGhostClick } from "../ghostClick";
  import DatePicker from "../DatePicker.svelte";
  import MarkdownToolbar from "../editor/MarkdownToolbar.svelte";
  import ContextMenu from "../menu/ContextMenu.svelte";
  import MenuItem from "../menu/MenuItem.svelte";
  import MenuSeparator from "../menu/MenuSeparator.svelte";
  import CalendarCategoryPanel from "./CalendarCategoryPanel.svelte";
  import {
    addDays, buildRepeatPreset, CALENDAR_IMAGE_NODE, calendarToday, dayNumber, durationText,
    eventColors, matchRepeatPreset, reminderLabel, repeatLabel, REPEAT_PRESET_LABELS,
    weekdayOf, type RepeatPresetId
  } from "../calendar";
  import { fullDayLabel, relativeDayLabel } from "../diary";
  import { nowClock } from "../clock";
  import type { CalendarEditorTarget, CalendarRepeat, ReminderRule, RepeatFreq } from "../types";
  import { createBackGuard, isMobile } from "../platform";
  import { imeInset } from "../imeInset";

  export let target: CalendarEditorTarget;
  export let onClose: () => void = () => {};
  export let onOpenLink: (url: string, title?: string) => void = () => {};

  const editingId = "id" in target ? target.id : null;
  const existing = editingId ? get(calendarEvents).find((event) => event.id === editingId) : undefined;
  const linkedTaskId0 = existing?.taskId || ("taskId" in target ? target.taskId ?? "" : "");
  const linkedTask0 = linkedTaskId0 ? get(appState).tasks.find((task) => task.id === linkedTaskId0) : undefined;

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

  function plusOneHour(date: string, time: string): { date: string; time: string } {
    const total = Number(time.slice(0, 2)) * 60 + Number(time.slice(3, 5)) + 60;
    return {
      date: total >= 1440 ? addDaysLocal(date, 1) : date,
      time: `${`${Math.floor((total % 1440) / 60)}`.padStart(2, "0")}:${`${(total % 1440) % 60}`.padStart(2, "0")}`
    };
  }

  function addDaysLocal(date: string, days: number): string {
    const [y, m, d] = date.split("-").map(Number);
    const dt = new Date(Date.UTC(y, m - 1, d + days));
    return `${dt.getUTCFullYear()}-${`${dt.getUTCMonth() + 1}`.padStart(2, "0")}-${`${dt.getUTCDate()}`.padStart(2, "0")}`;
  }

  function minutesFromClock(clock: string): number {
    return Number(clock.slice(0, 2)) * 60 + Number(clock.slice(3, 5));
  }

  function diffDaysLocal(from: string, to: string): number {
    const [fy, fm, fd] = from.split("-").map(Number);
    const [ty, tm, td] = to.split("-").map(Number);
    return Math.round((Date.UTC(ty, tm - 1, td) - Date.UTC(fy, fm - 1, fd)) / 86_400_000);
  }

  // ---- 初值 ----

  const start0 = ((): { date: string; time: string; allDay: boolean } => {
    if ("id" in target) {
      return {
        date: existing?.startDate ?? calendarToday(),
        time: existing?.startTime ?? "",
        allDay: existing?.allDay ?? false
      };
    }
    return {
      date: target.startDate,
      time: target.allDay ? "" : target.startTime ?? nextFullHour(),
      allDay: target.allDay === true
    };
  })();

  let host: HTMLDivElement;
  let imageFileInput: HTMLInputElement;
  let titleInput: HTMLInputElement;
  let view: EditorView | null = null;
  let saving = false;
  let closed = false;

  let title = existing?.title ?? (linkedTask0 ? taskSummary(linkedTask0.markdown) : "");
  let allDay = start0.allDay;
  let startDate = start0.date;
  let startTime = allDay ? "" : start0.time || nextFullHour();
  let endDate = existing?.endDate ?? ("endDate" in target ? target.endDate ?? startDate : startDate);
  let endTime = existing?.endTime ?? ("endTime" in target ? target.endTime ?? "" : "");
  if (!existing && !("endDate" in target && target.endDate) && !allDay) {
    const next = plusOneHour(startDate, startTime);
    endDate = next.date;
    endTime = next.time;
  }
  let reminders: ReminderRule[] = existing ? existing.reminders.map((rule) => ({ ...rule })) : [];
  let location = existing?.location ?? "";
  let categoryId = existing?.categoryId ?? (editingId ? "" : defaultCalendarCategoryId());
  let repeat: CalendarRepeat | null = existing?.repeat ? { ...existing.repeat } : null;
  let taskId = linkedTaskId0;
  let text = existing ? existing.details : (linkedTask0 ? linkedTask0.markdown : "");

  const initial = {
    title,
    allDay,
    startDate,
    startTime,
    endDate,
    endTime,
    text,
    location,
    categoryId,
    repeat: JSON.stringify(repeat),
    reminders: JSON.stringify(reminders),
    taskId
  };

  // ---- 详情区块（移动端懒挂载：不点进去就不唤起输入法） ----

  /** 详情是否处于「可编辑」状态：桌面一进来就是，移动端要用户点一下才是 */
  let detailsActive = !get(isMobile);
  let mode: "edit" | "preview" = "edit";
  let previewHtml = "";

  // ---- 浮层（一律挂在 ContextMenu 里） ----

  type PickerKind = "" | "start" | "end" | "repeat" | "reminder" | "category";
  let picker: PickerKind = "";
  let pickerAt = { x: 0, y: 0 };
  let pickerAnchor: HTMLElement | null = null;
  let showCategories = false;

  /** 自定义重复的草稿（点「应用」才写进 repeat） */
  let customFreq: RepeatFreq = "week";
  let customInterval = 1;
  let customWeekdays: number[] = [];
  let customMonthDays: number[] = [];
  let customEnd: CalendarRepeat["end"] = "never";
  let customCount = 10;
  let customUntil = calendarToday();
  /** 自定义提醒时刻的草稿 */
  let reminderDate = calendarToday();
  let reminderTime = nowClock();

  const REPEAT_PRESETS: Array<{ label: string; minutes: number }> = [
    { label: "日程开始时", minutes: 0 },
    { label: "提前 5 分钟", minutes: 5 },
    { label: "提前 15 分钟", minutes: 15 },
    { label: "提前 30 分钟", minutes: 30 },
    { label: "提前 1 小时", minutes: 60 },
    { label: "提前 2 小时", minutes: 120 },
    { label: "提前 1 天", minutes: 1440 }
  ];
  const WEEKDAY_LABELS = ["日", "一", "二", "三", "四", "五", "六"];
  const FREQS: Array<{ id: RepeatFreq; label: string; unit: string }> = [
    { id: "day", label: "天", unit: "天" },
    { id: "week", label: "周", unit: "周" },
    { id: "month", label: "月", unit: "个月" },
    { id: "year", label: "年", unit: "年" }
  ];
  const MONTH_DAYS = Array.from({ length: 31 }, (_, index) => index + 1);

  // ---- 派生 ----

  $: today = calendarToday();
  $: categories = $calendarCategories;
  $: accent = calendarAccent($appSettings.calendar);
  $: category = categoryId ? categories.find((item) => item.id === categoryId) : undefined;
  /** 分类色点预览：没选分类（或分类没配色）就跟日程主题色 */
  $: categoryColors = eventColors({ categoryId: "" }, [], category?.color || accent);
  // 依赖一律字面写在表达式里（`today` / `startDate`）：reactive 箭头函数会把依赖藏进闭包
  $: startText = allDay
    ? dayLabel(startDate, today)
    : `${dayLabel(startDate, today)} · ${startTime || "--:--"}`;
  $: endText = allDay ? dayLabel(endDate, today) : `${dayLabel(endDate, today)} · ${endTime || "--:--"}`;
  $: duration = durationText({
    startDate,
    startTime: allDay ? "" : startTime,
    endDate,
    endTime: allDay ? "" : endTime,
    allDay
  });
  $: repeatText = repeat ? repeatLabel(repeat) : "不重复";
  $: repeatPreset = matchRepeatPreset(repeat, startDate);
  $: linkedTask = taskId ? $appState.tasks.find((task) => task.id === taskId) : undefined;
  $: rangeError = validateRange();
  // 桌面编辑器尺寸按窗口比例（设置项）；移动端固定铺满不给内联值
  $: editorSizeStyle = $isMobile
    ? ""
    : `width: ${$appSettings.appearance.editorWidthPercent}%; height: ${$appSettings.appearance.editorHeightPercent}%;`;
  $: if (mode === "preview" || !detailsActive) {
    previewHtml = renderMarkdown(resolveMarkdownImages(text, CALENDAR_IMAGE_NODE, $mdImageCache));
  }

  /** "今天 · 10月27日 周二" / "3 天后 · 10月30日 周五" */
  function dayLabel(date: string, todayDate: string): string {
    return date === todayDate
      ? `今天 · ${fullDayLabel(date)}`
      : `${relativeDayLabel(date, todayDate)} · ${fullDayLabel(date)}`;
  }

  function validateRange(): string {
    if (endDate < startDate) return "结束日期不能早于开始日期";
    if (!allDay && endDate === startDate && endTime !== "" && endTime < startTime) {
      return "结束时间不能早于开始时间";
    }
    return "";
  }

  // ---- 生命周期 ----

  onMount(() => {
    if (detailsActive) ensureEditor(editingId !== null);
    // 移动端一律不自动聚焦：一进页面就弹输入法是需求 4 明确点名不要的
    if (!editingId && !get(isMobile)) void tick().then(() => titleInput?.focus());
  });

  // 外部路径卸载（移动端硬件返回弹历史栈）时未保存内容不能丢：尽力保存一次
  onDestroy(() => {
    if (closed || saving) return;
    void persist(true);
  });

  /** 详情编辑器按需挂载：移动端只有点了详情区块才建 CM（否则一进页面就是编辑态） */
  function ensureEditor(focus: boolean): void {
    detailsActive = true;
    if (mode !== "edit") mode = "edit";
    void tick().then(() => {
      if (!host || view) {
        if (focus) view?.focus();
        return;
      }
      view = createMarkdownEditor(host, text, {
        placeholder: taskId ? "详细信息与关联条目内容双向同步……" : "详细信息（Markdown）……",
        onSave: () => void saveAndClose(true),
        onClose: () => void saveAndClose(true),
        onChange: (value) => (text = value),
        onPasteImage: (file, editor) => void pasteImage(file, editor)
      });
      if (focus) view.focus();
    });
  }

  function currentText(): string {
    return view ? view.state.doc.toString() : text;
  }

  // ---- 保存 ----

  /** 落盘（新建或修改）。`implicit` = Esc/遮罩/返回键触发的保存：全空的新建直接放弃。 */
  async function persist(implicit: boolean): Promise<void> {
    const details = currentText();
    const trimmedTitle = title.trim();
    if (!editingId && implicit && !trimmedTitle && !details.trim()) return;
    const error = validateRange();
    if (error) {
      showToast(error);
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
      if (location !== initial.location) changes.location = location;
      if (categoryId !== initial.categoryId) changes.categoryId = categoryId;
      if (JSON.stringify(repeat) !== initial.repeat) changes.repeat = repeat;
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
      location,
      categoryId,
      repeat,
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

  // ---- 浮层开关 ----

  /**
   * 行里的浮层：锚在**触发按钮正下方、左缘对齐**（与全应用的点锚定菜单同一套约定），
   * 定位与钳制交给 ContextMenu——它是唯一一份浮层几何实现。
   */
  function togglePicker(kind: Exclude<PickerKind, "">, trigger: HTMLElement): void {
    if (picker === kind) {
      picker = "";
      return;
    }
    const rect = trigger.getBoundingClientRect();
    pickerAt = { x: rect.left, y: rect.bottom + 6 };
    pickerAnchor = trigger;
    picker = kind;
    if (kind === "repeat") seedCustomRepeat();
  }

  function closePicker(): void {
    picker = "";
    pickerAnchor = null;
  }

  // 返回键先收浮层，再按一次才轮到历史栈弹编辑器
  const backGuard = createBackGuard();
  $: backGuard(picker !== "", closePicker);

  // ---- 时间 ----

  function toggleAllDay(): void {
    allDay = !allDay;
    if (allDay) {
      endTime = "";
      return;
    }
    if (!startTime) startTime = nextFullHour();
    if (!endTime) {
      const next = plusOneHour(startDate, startTime);
      endDate = next.date;
      endTime = next.time;
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
      const next = plusOneHour(startDate, startTime);
      endDate = next.date;
      endTime = next.time;
    }
  }

  function pickStartDate(date: string): void {
    startDate = date;
    bumpEndAfterStartChange();
  }

  function pickStartTime(time: string): void {
    startTime = time;
    bumpEndAfterStartChange();
  }

  // ---- 重复 ----

  function seedCustomRepeat(): void {
    const source = repeat ?? buildRepeatPreset("weekly", startDate);
    if (!source) return;
    customFreq = source.freq;
    customInterval = source.interval;
    customWeekdays = source.byWeekday.length > 0 ? [...source.byWeekday] : [weekdayOf(startDate)];
    customMonthDays = source.byMonthDay.length > 0 ? [...source.byMonthDay] : [dayNumber(startDate)];
    customEnd = source.end;
    customCount = source.count > 0 ? source.count : 10;
    customUntil = source.until || addDays(startDate, 90);
  }

  function applyPreset(id: RepeatPresetId): void {
    repeat = buildRepeatPreset(id, startDate);
    closePicker();
  }

  function toggleWeekday(day: number): void {
    customWeekdays = customWeekdays.includes(day)
      ? customWeekdays.filter((item) => item !== day)
      : [...customWeekdays, day].sort((a, b) => a - b);
  }

  function toggleMonthDay(day: number): void {
    customMonthDays = customMonthDays.includes(day)
      ? customMonthDays.filter((item) => item !== day)
      : [...customMonthDays, day].sort((a, b) => a - b);
  }

  function applyCustomRepeat(): void {
    const next: CalendarRepeat = {
      freq: customFreq,
      interval: Math.min(999, Math.max(1, Math.round(customInterval) || 1)),
      byWeekday: customFreq === "week" ? customWeekdays : [],
      byMonthDay: customFreq === "month" || customFreq === "year" ? customMonthDays : [],
      end: customEnd,
      count: customEnd === "count" ? Math.min(999, Math.max(1, Math.round(customCount) || 1)) : 0,
      until: customEnd === "until" ? customUntil : ""
    };
    if (next.freq === "week" && next.byWeekday.length === 0) {
      showToast("至少选一个星期");
      return;
    }
    if (next.freq === "month" && next.byMonthDay.length === 0) {
      showToast("至少选一个日期");
      return;
    }
    if (next.end === "until" && next.until < startDate) {
      showToast("截止日期不能早于开始日期");
      return;
    }
    repeat = next;
    closePicker();
  }

  // ---- 提醒 ----

  function addBeforeReminder(minutes: number): void {
    if (reminders.some((rule) => rule.kind === "beforeDue" && rule.minutes === minutes)) {
      showToast("这条提醒已经加过了");
      return;
    }
    reminders = [...reminders, { kind: "beforeDue", minutes }];
  }

  function addCustomReminder(): void {
    const at = new Date(`${reminderDate}T${reminderTime}:00`);
    if (Number.isNaN(at.getTime())) {
      showToast("自定义提醒时刻无效");
      return;
    }
    if (at.getTime() <= Date.now()) {
      showToast("提醒时刻必须在未来");
      return;
    }
    const iso = at.toISOString();
    if (reminders.some((rule) => rule.kind === "absolute" && rule.at === iso)) {
      showToast("这条提醒已经加过了");
      return;
    }
    reminders = [...reminders, { kind: "absolute", at: iso }];
    closePicker();
  }

  function removeReminder(index: number): void {
    reminders = reminders.filter((_, position) => position !== index);
  }

  function unlinkTask(): void {
    taskId = "";
  }

  function openCategoryManager(): void {
    closePicker();
    showCategories = true;
  }

  // ---- 详情插图（走 calendar 伪条目通道，与日记同一套） ----

  async function insertImageReference(filename: string): Promise<void> {
    const url = await mdImageUrl(CALENDAR_IMAGE_NODE, filename);
    primeMdImageCache(CALENDAR_IMAGE_NODE, filename, url);
    if (view) insertAtCursor(view, `\n![](${filename})\n`);
  }

  async function insertImageFile(): Promise<void> {
    if (!isTauriRuntime) return;
    ensureEditor(true);
    await tick();
    if (!view) return;
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
    if (!isTauriRuntime) return;
    ensureEditor(true);
    await tick();
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

  function toggleMode(next: "edit" | "preview"): void {
    if (next === "edit") {
      ensureEditor(true);
      return;
    }
    text = currentText();
    mode = "preview";
    picker = "";
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
      // ContextMenu 自己吃 Esc（window 捕获 + stopPropagation），走不到这里
      event.preventDefault();
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
  <!-- --accent 内联：编辑器浮层挂在 App 层，拿不到 .calendar-view 的主题色 -->
  <div
    class="editor-dialog calendar-editor calendar-form"
    style={`--accent: ${accent}; ${editorSizeStyle}`}
    role="dialog"
    aria-label={editingId ? "编辑日程" : "新建日程"}
    tabindex="-1"
    on:pointerdown|stopPropagation
    on:click|stopPropagation
  >
    <header class="editor-header">
      <span class="editor-title">{editingId ? "编辑日程" : "新建日程"}</span>
      <div class="editor-actions">
        {#if editingId}
          <button class="editor-icon-button danger" type="button" title="删除日程" disabled={saving} on:click={() => void removeAndClose()}>
            <Trash2 size={17} />
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

    <div class="calendar-form-body">
      <input
        bind:this={titleInput}
        bind:value={title}
        class="editor-title-input calendar-title-input"
        type="text"
        maxlength="200"
        placeholder="日程名称"
        on:keydown={(event) => { if (event.key === "Enter" && !event.isComposing && event.keyCode !== 229) { event.preventDefault(); ensureEditor(true); } }}
      />

      {#if taskId}
        <div class="cal-field">
          <span class="cal-field-label">关联条目</span>
          <span class="cal-field-static">
            <span class="task-tag tag-purple calendar-linked-chip">
              <PenLine size={11} />
              {linkedTask ? taskSummary(linkedTask.markdown) : "关联条目"}
              <button class="tag-delete" type="button" aria-label="解除关联" on:click|stopPropagation={unlinkTask}>
                <Link2Off size={10} strokeWidth={3} />
              </button>
            </span>
            <span class="cal-field-note">详情与条目内容双向同步</span>
          </span>
        </div>
      {/if}

      <div class="cal-field">
        <span class="cal-field-label" id="cal-allday-label">全天</span>
        <span class="cal-field-control">
          <label class="toggle-row cal-toggle" title="全天日程不占时间网格，显示在日/周视图顶部的全天栏">
            <span aria-hidden="true">整天，不指定时刻</span>
            <input type="checkbox" checked={allDay} on:change={toggleAllDay} aria-labelledby="cal-allday-label" />
          </label>
        </span>
      </div>

      <div class="cal-field">
        <span class="cal-field-label">开始</span>
        <button
          type="button"
          class="cal-field-trigger"
          aria-expanded={picker === "start"}
          on:click={(event) => togglePicker("start", event.currentTarget)}
        >
          <CalendarDays size={15} />
          <span class="cal-field-value">{startText}</span>
          <ChevronRight size={15} class="cal-field-chevron" />
        </button>
      </div>

      <div class="cal-field">
        <span class="cal-field-label">结束</span>
        <button
          type="button"
          class="cal-field-trigger"
          class:disabled={allDay && endDate === startDate}
          aria-expanded={picker === "end"}
          on:click={(event) => togglePicker("end", event.currentTarget)}
        >
          <CalendarDays size={15} />
          <span class="cal-field-value">{endText}</span>
          <ChevronRight size={15} class="cal-field-chevron" />
        </button>
      </div>

      {#if duration}
        <div class="cal-field">
          <span class="cal-field-label">时长</span>
          <span class="cal-field-static"><span class="calendar-duration">{duration}</span></span>
        </div>
      {/if}

      <div class="cal-field">
        <span class="cal-field-label">重复</span>
        <button
          type="button"
          class="cal-field-trigger"
          class:on={repeat !== null}
          aria-expanded={picker === "repeat"}
          on:click={(event) => togglePicker("repeat", event.currentTarget)}
        >
          <RepeatIcon size={15} />
          <span class="cal-field-value">{repeatText}</span>
          <ChevronRight size={15} class="cal-field-chevron" />
        </button>
      </div>

      <div class="cal-field cal-field-wrap">
        <span class="cal-field-label">提醒</span>
        <span class="cal-field-static calendar-reminder-list">
          {#each reminders as rule, index (`${rule.kind}-${index}`)}
            <span class="task-tag tag-blue calendar-reminder-chip" title={reminderLabel(rule)}>
              <AlarmClock size={11} />
              {reminderLabel(rule)}
              <button class="tag-delete" type="button" aria-label="移除提醒" on:click|stopPropagation={() => removeReminder(index)}>
                <X size={10} strokeWidth={3} />
              </button>
            </span>
          {/each}
          <button
            type="button"
            class="cal-field-trigger compact"
            aria-expanded={picker === "reminder"}
            on:click={(event) => togglePicker("reminder", event.currentTarget)}
          >
            <Bell size={14} />
            <span class="cal-field-value">{reminders.length === 0 ? "添加提醒" : "继续添加"}</span>
          </button>
        </span>
      </div>

      <div class="cal-field">
        <span class="cal-field-label">地点</span>
        <span class="cal-field-control cal-field-location">
          <MapPin size={15} />
          <input
            type="text"
            maxlength="120"
            placeholder="会议室 / 地址（可留空）"
            bind:value={location}
          />
        </span>
      </div>

      <div class="cal-field">
        <span class="cal-field-label">分类</span>
        <button
          type="button"
          class="cal-field-trigger"
          aria-expanded={picker === "category"}
          on:click={(event) => togglePicker("category", event.currentTarget)}
        >
          <span class="calendar-dot" style={`--dot: ${categoryColors.main}; --dot-soft: ${categoryColors.soft}`}></span>
          <span class="cal-field-value">{category ? category.name : "无分类"}</span>
          <ChevronRight size={15} class="cal-field-chevron" />
        </button>
      </div>

      {#if rangeError}
        <p class="calendar-form-error">{rangeError}</p>
      {/if}

      <section class="calendar-details" class:active={detailsActive && mode === "edit"}>
        <header class="calendar-details-head">
          <span class="calendar-details-title">详细信息</span>
          <div class="editor-mode-switch compact" role="tablist" data-mode={detailsActive ? mode : "preview"}>
            <span class="editor-mode-thumb" aria-hidden="true"></span>
            <button type="button" role="tab" class:active={detailsActive && mode === "edit"} aria-selected={detailsActive && mode === "edit"} on:click={() => toggleMode("edit")}>
              <PenLine size={14} />编辑
            </button>
            <button type="button" role="tab" class:active={!detailsActive || mode === "preview"} aria-selected={!detailsActive || mode === "preview"} on:click={() => toggleMode("preview")}>
              <Eye size={14} />预览
            </button>
          </div>
          {#if isTauriRuntime}
            <button class="editor-icon-button" type="button" title="插入图片" on:click={() => void insertImageFile()}>
              <ImagePlus size={16} />
            </button>
          {/if}
        </header>

        {#if detailsActive}
          {#if $appSettings.features.editorToolbar && mode === "edit"}
            <MarkdownToolbar view={view} onImage={() => void insertImageFile()} />
          {/if}
          <!-- 挂过 CodeMirror 之后宿主**必须一直挂着**（只是藏起来）：Svelte 把 div 销毁后
               `view` 仍非空，ensureEditor 就不会重建，编辑器会连着一个脱离文档的节点。 -->
          <div class="calendar-details-body" class:hidden-body={mode === "preview"}>
            <div bind:this={host} class="editor-cm-host"></div>
          </div>
        {/if}

        {#if mode === "preview" || !detailsActive}
          {#if text.trim() !== ""}
            <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
            <div class="calendar-details-body" on:click={() => { if (!detailsActive) ensureEditor(true); }}>
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="markdown-body markdown-content editor-preview" use:markdownWire on:click={handlePreviewClick}>
                {@html previewHtml}
              </div>
            </div>
          {:else if !detailsActive}
            <!-- 移动端默认停在这一格：点一下才挂编辑器（不自动唤起输入法） -->
            <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
            <div class="calendar-details-empty" on:click={() => ensureEditor(true)} role="button" tabindex="0">
              点击添加详细信息（支持 Markdown）
            </div>
          {/if}
        {/if}
      </section>
    </div>

    <input bind:this={imageFileInput} class="hidden-file" type="file" accept="image/*" on:change={insertImageFromInput} />
  </div>

  {#if picker === "start" || picker === "end"}
    <ContextMenu
      x={pickerAt.x}
      y={pickerAt.y}
      xAlign="left"
      mirrorXOnFlip={false}
      minWidth={252}
      anchor={pickerAnchor}
      onClose={closePicker}
    >
      <div class="calendar-pop-host" on:click|stopPropagation on:mousedown|stopPropagation>
        <DatePicker
          value={picker === "start" ? startDate : endDate}
          time={allDay ? "" : (picker === "start" ? startTime : endTime)}
          withTime={!allDay}
          confirmMode
          allowClear={false}
          on:select={(event) => { if (picker === "start") pickStartDate(event.detail); else endDate = event.detail; }}
          on:selectTime={(event) => { if (picker === "start") pickStartTime(event.detail); else endTime = event.detail; }}
          on:clear={() => undefined}
          on:close={closePicker}
        />
      </div>
    </ContextMenu>
  {:else if picker === "repeat"}
    <ContextMenu x={pickerAt.x} y={pickerAt.y} xAlign="left" mirrorXOnFlip={false} minWidth={236} anchor={pickerAnchor} onClose={closePicker}>
      {#each Object.entries(REPEAT_PRESET_LABELS) as [id, label] (id)}
        <MenuItem
          checkable
          active={repeatPreset === id}
          label={label}
          onSelect={() => applyPreset(id as RepeatPresetId)}
        />
      {/each}
      <MenuSeparator />
      <MenuItem icon={SlidersHorizontal} label="自定义…">
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <div slot="submenu" class="calendar-repeat-custom" on:click|stopPropagation>
          <div class="calendar-custom-row">
            <span>频率</span>
            <span class="segmented">
              {#each FREQS as freq (freq.id)}
                <button type="button" class:active={customFreq === freq.id} on:click={() => (customFreq = freq.id)}>{freq.label}</button>
              {/each}
            </span>
          </div>
          <div class="calendar-custom-row">
            <span>每</span>
            <input
              class="calendar-custom-number"
              type="number"
              min="1"
              max="999"
              bind:value={customInterval}
              on:keydown|stopPropagation
            />
            <span>{FREQS.find((item) => item.id === customFreq)?.unit ?? "天"}</span>
          </div>
          {#if customFreq === "week"}
            <div class="calendar-custom-row calendar-custom-column">
              <span>星期</span>
              <span class="calendar-weekday-row">
                {#each WEEKDAY_LABELS as label, index (index)}
                  <button
                    type="button"
                    class="calendar-weekday"
                    class:active={customWeekdays.includes(index)}
                    on:click={() => toggleWeekday(index)}
                  >{label}</button>
                {/each}
              </span>
            </div>
          {/if}
          {#if customFreq === "month" || customFreq === "year"}
            <div class="calendar-custom-row calendar-custom-column">
              <span>{customFreq === "year" ? `${Number(startDate.slice(5, 7))} 月的日期` : "日期"}</span>
              <span class="calendar-monthday-grid">
                {#each MONTH_DAYS as day (day)}
                  <button
                    type="button"
                    class="calendar-monthday"
                    class:active={customMonthDays.includes(day)}
                    on:click={() => toggleMonthDay(day)}
                  >{day}</button>
                {/each}
              </span>
            </div>
          {/if}
          <div class="calendar-custom-row calendar-custom-column">
            <span>结束</span>
            <span class="calendar-end-row">
              <button type="button" class="menu-action-button" class:active={customEnd === "never"} on:click={() => (customEnd = "never")}>永不</button>
              <button type="button" class="menu-action-button" class:active={customEnd === "count"} on:click={() => (customEnd = "count")}>按次数</button>
              <button type="button" class="menu-action-button" class:active={customEnd === "until"} on:click={() => (customEnd = "until")}>按日期</button>
            </span>
            {#if customEnd === "count"}
              <span class="calendar-end-detail">
                共
                <input class="calendar-custom-number" type="number" min="1" max="999" bind:value={customCount} on:keydown|stopPropagation />
                次（含首次）
              </span>
            {:else if customEnd === "until"}
              <span class="calendar-end-detail">
                到
                <input type="date" bind:value={customUntil} on:keydown|stopPropagation />
                为止（含当天）
              </span>
            {/if}
          </div>
          <button class="menu-action-button primary calendar-custom-apply" type="button" on:click={applyCustomRepeat}>
            <Check size={14} />应用自定义重复
          </button>
        </div>
      </MenuItem>
    </ContextMenu>
  {:else if picker === "reminder"}
    <ContextMenu x={pickerAt.x} y={pickerAt.y} xAlign="left" mirrorXOnFlip={false} minWidth={236} anchor={pickerAnchor} onClose={closePicker}>
      {#each REPEAT_PRESETS as preset (preset.minutes)}
        <MenuItem label={preset.label} onSelect={() => addBeforeReminder(preset.minutes)} />
      {/each}
      <MenuSeparator />
      <MenuItem icon={AlarmClock} label="指定具体时刻…">
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <div slot="submenu" class="calendar-reminder-custom" on:click|stopPropagation>
          <p class="calendar-custom-hint">在某个绝对时刻提醒（与日程开始时间无关）。必须在未来。</p>
          <label class="calendar-custom-row">
            <span>日期</span>
            <input type="date" bind:value={reminderDate} on:keydown|stopPropagation />
          </label>
          <label class="calendar-custom-row">
            <span>时刻</span>
            <input type="time" bind:value={reminderTime} on:keydown|stopPropagation />
          </label>
          <button class="menu-action-button primary calendar-custom-apply" type="button" on:click={addCustomReminder}>
            <Check size={14} />添加这条提醒
          </button>
        </div>
      </MenuItem>
    </ContextMenu>
  {:else if picker === "category"}
    <ContextMenu x={pickerAt.x} y={pickerAt.y} xAlign="left" mirrorXOnFlip={false} minWidth={216} anchor={pickerAnchor} onClose={closePicker}>
      <MenuItem
        checkable
        active={categoryId === ""}
        label="无分类（用日程主题色）"
        onSelect={() => { categoryId = ""; closePicker(); }}
      />
      {#each categories as item (item.id)}
        <MenuItem
          checkable
          active={categoryId === item.id}
          label={item.isDefault ? `${item.name} · 默认` : item.name}
          onSelect={() => { categoryId = item.id; closePicker(); }}
        />
      {/each}
      <MenuSeparator />
      <MenuItem icon={Tags} label="管理分类…" onSelect={openCategoryManager} />
    </ContextMenu>
  {/if}

  {#if showCategories}
    <CalendarCategoryPanel onClose={() => (showCategories = false)} />
  {/if}
</div>
