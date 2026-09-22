<script lang="ts">
  import { onMount, tick } from "svelte";
  import {
    Calendar, CalendarDays, ChevronDown, ChevronLeft, ChevronRight,
    ChevronsDown, ChevronsUp, ExternalLink, FolderInput,
    Lightbulb, MoreHorizontal, PenLine, Plus, RefreshCw, Search, Settings as SettingsIcon, SmilePlus, Star, Sun, Tag, Trash2, X
  } from "@lucide/svelte";
  import {
    appState, appSettings, showToast,
    searchQuery, searchHits, searchScanning, selectedNode, visibleTasks, selectedBackground,
    accent, isSearching, todayIso, yesterdayIso, dateOnly,
    taskEmojiPicker, editorTaskId, editorDraftNode, diaryEditor, diaryEntries, ledgerData, ledgerEditor,
    fileToDataUrl, weekStart
  } from "./stores";
  import {
    updateTask as updateTaskAction, deleteTask as deleteTaskAction,
    setTaskSchedule as setTaskScheduleAction,
    addTask as addTaskAction, setItemUi as setItemUiAction,
    setItemsUi as setItemsUiAction, replaceTaskTags as replaceTaskTagsAction,
    replaceTaskEmojis as replaceTaskEmojisAction,
    setDiaryUi as setDiaryUiAction,
    renameNode as renameNodeAction, syncNow as syncNowAction
  } from "./actions";
  import { pullToRefresh } from "./pullrefresh";
  import { taskMoveTargets } from "./nodes";
  import { buildMainStyle, ledgerAccent, PAGE_HEADER_ICON_SIZE } from "./styles";
  import { accentWithPreview, backgroundWithPreview, colorPreview } from "./colorPreview";
  import { hasMultipleMarkdownLines } from "./markdown";
  import { openExternalUrl, isTauriRuntime, saveMdImageFromDataUrl, mdImageUrl } from "./backend";
  import { imageCache, resolveImageSrc, mdImageCache, primeMdImageCache } from "./images";
  import IconGlyph from "./IconGlyph.svelte";
  import MobileBack from "./MobileBack.svelte";
  import TaskCard from "./TaskCard.svelte";
  import VirtualStack from "./VirtualStack.svelte";
  import DiaryCard from "./diary/DiaryCard.svelte";
  import DiaryEntryMenu from "./diary/DiaryEntryMenu.svelte";
  import LedgerEntryCard from "./ledger/LedgerEntryCard.svelte";
  import ScheduledTasksView from "./ScheduledTasksView.svelte";
  import TaskDateReminderPanel from "./TaskDateReminderPanel.svelte";
  import ContextMenu from "./menu/ContextMenu.svelte";
  import MenuItem from "./menu/MenuItem.svelte";
  import MonthPopover from "./MonthPopover.svelte";
  import MenuSeparator from "./menu/MenuSeparator.svelte";
  import MoveTargetTree from "./menu/MoveTargetTree.svelte";
  import ListMenu from "./workspace/ListMenu.svelte";
  import TagMenuPanel from "./TagMenuPanel.svelte";
  import { sortTasks, type SortMode } from "./sort";
  import { filterPlannedTasks, plannedGroupOptions, plannedSections, type PlannedGroupKey } from "./plannedGroups";
  import { calendarWeekdayHeaders } from "./diary";
  import { createBackGuard, isMobile, mobileView } from "./platform";
  import { caps } from "./capabilities";
  import type { AppNode, CardStyle, ReminderRule, SearchHit, TagColor, Task } from "./types";

  // 「已完成」区显隐偏好：默认折叠，用户配置过就按视图记住（本机 UI 状态，不进同步）
  const COMPLETED_OPEN_KEY = "kxtodo-completed-open";
  const COMPLETED_PLANNED_KEY = "planned";

  function readCompletedOpen(key: string): boolean {
    if (typeof localStorage === "undefined") return false;
    try {
      const raw = localStorage.getItem(COMPLETED_OPEN_KEY);
      if (!raw) return false;
      return (JSON.parse(raw) as Record<string, boolean>)[key] === true;
    } catch {
      return false;
    }
  }

  function writeCompletedOpen(key: string, open: boolean): void {
    if (typeof localStorage === "undefined") return;
    try {
      const raw = localStorage.getItem(COMPLETED_OPEN_KEY);
      const map = raw ? (JSON.parse(raw) as Record<string, boolean>) : {};
      map[key] = open;
      localStorage.setItem(COMPLETED_OPEN_KEY, JSON.stringify(map));
    } catch {
      // 写不进就算了，最坏退回每次默认折叠
    }
  }

  let newTaskDraft = "";
  /** 「已完成」区默认折叠；用户展开/收起后按当前视图记住（见 COMPLETED_OPEN_KEY） */
  let showCompleted = false;
  let showSuggestions = false;
  let showCalendar = false;
  let sortMode: SortMode = "created-desc";
  // 计划内视图：日期分组过滤 + 已完成显隐（默认隐藏，且不渲染折叠的已完成区）
  let plannedGroup: PlannedGroupKey = "all";
  let plannedShowCompleted = readCompletedOpen(COMPLETED_PLANNED_KEY);
  let showPlannedGroups = false;
  /** 计划内「全部」的分区折叠状态（本地 UI 状态，不持久化） */
  let collapsedSections: Record<string, boolean> = {};

  function toggleSection(key: string): void {
    collapsedSections = { ...collapsedSections, [key]: !collapsedSections[key] };
  }

  /** 换视图时回读该视图的「已完成」显隐偏好（没配置过 = 折叠） */
  $: completedKey = $selectedNode?.id ?? "list";
  $: showCompleted = readCompletedOpen(completedKey);

  function toggleCompletedSection(): void {
    showCompleted = !showCompleted;
    writeCompletedOpen(completedKey, showCompleted);
  }

  function togglePlannedCompleted(): void {
    plannedShowCompleted = !plannedShowCompleted;
    writeCompletedOpen(COMPLETED_PLANNED_KEY, plannedShowCompleted);
  }
  let taskMenu: { taskId: string; x: number; y: number } | null = null;
  /** 搜索结果里的日记卡片菜单（与 taskMenu 互斥） */
  let diaryMenu: { id: string; x: number; y: number } | null = null;
  let listMenuAt: { x: number; y: number } | null = null;
  /** 菜单是三点按钮开的（true）还是齿轮面板转过来的（false）——前者的按钮要能 toggle */
  let listMenuFromButton = false;
  let listMenuButtonEl: HTMLButtonElement;
  let headerRenaming = false;
  let headerRenameDraft = "";
  let headerRenameInput: HTMLInputElement;
  let taskInput: HTMLTextAreaElement;
  let schedulerViewRef: ScheduledTasksView;
  let showHeaderMenu = false;
  let gearButtonEl: HTMLButtonElement;
  /** 任务列表的滚动容器（窗口化要用它当视口） */
  let listScrollEl: HTMLElement | null = null;
  let linkPreviewUrl = "";
  /** 内置浏览器顶部标题栏的文字：链接文字 → 同源时读到的网页标题 → 主机名兜底 */
  let linkPreviewTitle = "";
  let previewFrame: HTMLIFrameElement;
  // 分钟级 tick：让计划内分组标签（周X/日期区间）在跨天后随下次重算刷新
  let dayTick = 0;
  // 移动端下拉同步：提示条高度/是否过阈值/上一轮是否还在跑
  let pullDistance = 0;
  let pullReady = false;
  let pullBusy = false;

  /** 同步功能开着（总开关 + 已配对且没暂停）才给下拉手势与「立即同步」入口 */
  $: syncAvailable =
    $appSettings.features?.sync !== false &&
    Boolean($appSettings.sync?.enabled) &&
    Boolean(($appSettings.sync?.username ?? "").trim()) &&
    Boolean(($appSettings.sync?.secret ?? "").trim());

  async function runManualSyncNow(): Promise<void> {
    if (pullBusy) return;
    pullBusy = true;
    try {
      await syncNowAction();
    } finally {
      pullBusy = false;
    }
  }

  onMount(() => {
    const dayTimer = window.setInterval(() => {
      dayTick += 1;
    }, 60_000);
    return () => window.clearInterval(dayTimer);
  });

  // 离开内容视图（移动端回列表/设置/工具箱）时收起下拉，避免再进入时残留开面板
  $: if ($mobileView !== "content") {
    showHeaderMenu = false;
    showPlannedGroups = false;
  }

  /**
   * 被整页视图盖住（v0.8.6 需求 3）：日记/记账/工具箱改成不透明覆盖之后，工作区仍在
   * 树上（返回零闪烁的代价）——加 `inert` 摘出可访问性与交互；旧 WebKit 由 mobile.css
   * 的 `visibility: hidden` 兜底。
   */
  $: coveredByFullPage = $isMobile && ($mobileView === "diary" || $mobileView === "ledger" || $mobileView === "toolbox");

  function handlePanelKeydown(event: KeyboardEvent): void {
    if (!showHeaderMenu && !showPlannedGroups) return;
    if (event.key === "Escape" && !event.isComposing && event.keyCode !== 229) {
      showHeaderMenu = false;
      showPlannedGroups = false;
    }
  }

  // Calendar state
  let calViewMode: "month" | "week" = "month";
  let calYear = new Date().getFullYear();
  let calMonth = new Date().getMonth();
  let calPopOpen = false;
  let calLabelEl: HTMLElement;
  // The date whose completed tasks are mirrored in the main area (history view).
  // Always resets to today when leaving / re-entering My Day.
  let myDayViewDate = todayIso();

  $: resolvedBgImage = resolveImageSrc($selectedBackground.image, $imageCache);
  // 取色预览（需求 9）：菜单里拖色盘时先把界面染上，点保存才落盘
  $: previewScope = $selectedNode?.id ?? "";
  $: mainStyle = buildMainStyle(
    backgroundWithPreview($colorPreview, previewScope, $selectedBackground),
    accentWithPreview($colorPreview, previewScope, $accent),
    resolvedBgImage
  );
  $: isMyDay = $selectedNode?.id === "my-day";
  $: isPlanned = $selectedNode?.id === "planned" && !$isSearching;
  $: isScheduled = caps.scheduler && !$isSearching && $selectedNode?.id === "scheduled";
  $: if (!isMyDay && myDayViewDate !== todayIso()) myDayViewDate = todayIso();
  $: isMyDayHistory = isMyDay && myDayViewDate !== todayIso();
  $: sortedTasks = sortTasks($visibleTasks, sortMode);
  // dayTick 仅用于提供响应式依赖（每分钟重算一次标签，跨天不陈旧）
  $: plannedOptions = dayTick >= 0 ? plannedGroupOptions(todayIso(), $weekStart) : [];
  $: plannedGroupLabel = plannedOptions.find((option) => option.key === plannedGroup)?.label ?? "全部";
  $: plannedSortedTasks = isPlanned
    ? sortTasks(filterPlannedTasks($visibleTasks, plannedGroup, todayIso()), sortMode)
    : [];
  // 每张卡片按**自己所属条目**的分组类型渲染（我的一天/计划内/搜索里混着多个条目的任务）
  $: cardStyleByNode = new Map<string, CardStyle>(
    $appState.nodes.filter((node) => node.cardStyle === "card").map((node) => [node.id, "card"])
  );
  $: selectedIsCard = $selectedNode?.cardStyle === "card";
  $: incompleteTasks = isPlanned
    ? (plannedShowCompleted ? plannedSortedTasks : plannedSortedTasks.filter((task) => !task.completed))
    : isMyDayHistory
      ? []
      : selectedIsCard
        ? sortedTasks
        : sortedTasks.filter((task) => !task.completed);
  // 「已完成」按完成时间降序（最新完成在最上）；三点菜单的排序方式只管未完成部分。
  // 一般卡片条目不分区：已完成的当普通卡片混在主列表里显示。
  $: completedTasks = isPlanned || selectedIsCard
    ? []
    : isMyDay
      ? (isMyDayHistory
          ? byCompletedDesc(completedByDate[myDayViewDate] ?? [])
          : byCompletedDesc(sortedTasks.filter((task) => task.completed && dateOnly(task.completedAt) === todayIso())))
      : byCompletedDesc(sortedTasks.filter((task) => task.completed));
  // 「计划内·全部」分段展示：互斥分区（已逾期/近三天/本周/稍后），空分区不渲染
  $: plannedSectionList =
    isPlanned && plannedGroup === "all"
      ? plannedSections(
          plannedShowCompleted ? plannedSortedTasks : plannedSortedTasks.filter((task) => !task.completed),
          todayIso()
        )
      : [];
  /** 渲染行：分区标题与卡片拍平成同一条列表（窗口化要吃一份可索引的数据） */
  type TaskRow = {
    kind: "label" | "task";
    key: string;
    label: string;
    count: number;
    sectionKey: string;
    collapsed: boolean;
    task: Task | null;
  };

  // 渲染行：分区标题 + 卡片 拍平成一条列表，避免把 TaskCard 的接线复制第三遍；
  // 折叠的分区只留标题行（标题本身是折叠按钮）
  $: taskRows = (plannedSectionList.length
    ? plannedSectionList.flatMap((section) => {
        const collapsed = Boolean(collapsedSections[section.key]);
        return [
          {
            kind: "label" as const,
            key: `section-${section.key}`,
            label: section.label,
            count: section.tasks.length,
            sectionKey: section.key,
            collapsed,
            task: null as Task | null
          },
          ...(collapsed
            ? []
            : section.tasks.map((task) => ({
                kind: "task" as const,
                key: task.id,
                label: "",
                count: 0,
                sectionKey: "",
                collapsed: false,
                task: task as Task | null
              })))
        ];
      })
    : incompleteTasks.map((task): TaskRow => ({
        kind: "task",
        key: task.id,
        label: "",
        count: 0,
        sectionKey: "",
        collapsed: false,
        task
      }))) as TaskRow[];
  $: taskMenuTask = taskMenu ? $appState.tasks.find((task) => task.id === taskMenu?.taskId) : null;
  $: diaryMenuEntry = diaryMenu ? $diaryEntries.find((entry) => entry.id === diaryMenu?.id) ?? null : null;
  /** 记账搜索结果的 --accent：工作区里拿不到 LedgerView 的内联主题色，从设置算一份 */
  $: ledgerAccentColor = ledgerAccent($appSettings.ledger);
  $: hasTaskMoveTargets = taskMenu ? taskMoveTargets($appState.nodes, taskMenuTask?.nodeId ?? "").length > 0 : false;
  // 可展开集合 = 多行的 ∪ 卡片量出来「显示不全」的（单行超长也要折行，也算折叠块）。
  // 卡片在 TaskCard 里量，量完把自己的结论报上来（measure 事件）。
  // **必须在这条语句里直接读 measuredExpandable**：Svelte 不跟踪函数调用里的依赖，
  // 写成 `filter(isExpandable)` 的话注册表更新了这条语句也不会重算（按钮不跟着变）。
  $: expandableTasks = $visibleTasks.filter(
    (task) => hasMultipleMarkdownLines(task.markdown) || measuredExpandable.has(task.id)
  );
  $: allExpanded = expandableTasks.length > 0 && expandableTasks.every((task) => task.expanded);
  $: allCollapsed = expandableTasks.every((task) => !task.expanded);

  // My Day suggestions
  $: suggestedTasks = (() => {
    if (!isMyDay) return [];
    const today = todayIso();
    const yesterday = yesterdayIso();
    const candidates: Task[] = [];
    const seen = new Set<string>();
    for (const task of $appState.tasks) {
      if (task.completed || task.myDay || seen.has(task.id)) continue;
      const created = dateOnly(task.createdAt);
      const due = dateOnly(task.dueDate);
      if (created === today || created === yesterday || due === today) {
        candidates.push(task);
        seen.add(task.id);
      }
    }
    if (candidates.length > 0) return candidates;
    return $appState.tasks
      .filter((t) => !t.completed && !t.myDay)
      .sort((a, b) => b.createdAt.localeCompare(a.createdAt))
      .slice(0, 5);
  })();

  // Calendar: completed tasks by date
  $: completedByDate = (() => {
    const map: Record<string, Task[]> = {};
    for (const task of $appState.tasks) {
      if (!task.completed || !task.completedAt) continue;
      const d = task.completedAt.slice(0, 10);
      (map[d] ??= []).push(task);
    }
    return map;
  })();

  $: calSelectedTasks = completedByDate[myDayViewDate] ?? [];

  // Week view: a summary of the whole week's completed tasks grouped by day.
  $: weekSummary = (() => {
    const d = new Date(myDayViewDate + "T00:00:00");
    const start = new Date(d);
    start.setDate(start.getDate() - ((d.getDay() - $weekStart + 7) % 7));
    const days: Array<{ date: string; label: string; tasks: Task[] }> = [];
    for (let i = 0; i < 7; i++) {
      const cur = new Date(start);
      cur.setDate(cur.getDate() + i);
      const ds = fmtDateStr(cur.getFullYear(), cur.getMonth(), cur.getDate());
      days.push({
        date: ds,
        label: `${cur.getMonth() + 1}月${cur.getDate()}日 周${weekDayLabels[cur.getDay()]}`,
        tasks: completedByDate[ds] ?? []
      });
    }
    return days;
  })();

  $: weekSummaryTotal = weekSummary.reduce((sum, day) => sum + day.tasks.length, 0);

  // 一周从周几开始跟着设置走（默认周一）：表头、月历网格、周汇总、周区间同一个来源
  $: weekDayLabels = calendarWeekdayHeaders($weekStart);

  function formatMyDayDate(dateStr: string): string {
    const d = new Date(dateStr + "T00:00:00");
    const weekDays = ["星期日", "星期一", "星期二", "星期三", "星期四", "星期五", "星期六"];
    return `${d.getMonth() + 1}月${d.getDate()}日, ${weekDays[d.getDay()]}`;
  }

  function calMonthDays(): Array<{ date: string; day: number; current: boolean; hasTask: boolean }> {
    const first = new Date(calYear, calMonth, 1);
    const last = new Date(calYear, calMonth + 1, 0);
    const startDow = (first.getDay() - $weekStart + 7) % 7;
    const totalDays = last.getDate();
    const cells: Array<{ date: string; day: number; current: boolean; hasTask: boolean }> = [];
    const prevLast = new Date(calYear, calMonth, 0);
    for (let i = startDow - 1; i >= 0; i--) {
      const dd = prevLast.getDate() - i;
      const ds = fmtDateStr(calYear, calMonth - 1, dd);
      cells.push({ date: ds, day: dd, current: false, hasTask: !!completedByDate[ds]?.length });
    }
    for (let dd = 1; dd <= totalDays; dd++) {
      const ds = fmtDateStr(calYear, calMonth, dd);
      cells.push({ date: ds, day: dd, current: true, hasTask: !!completedByDate[ds]?.length });
    }
    const rem = (7 - (cells.length % 7)) % 7;
    for (let dd = 1; dd <= rem; dd++) {
      const ds = fmtDateStr(calYear, calMonth + 1, dd);
      cells.push({ date: ds, day: dd, current: false, hasTask: !!completedByDate[ds]?.length });
    }
    return cells;
  }

  function calWeekRange(): string {
    const d = new Date(myDayViewDate + "T00:00:00");
    const start = new Date(d);
    start.setDate(start.getDate() - ((d.getDay() - $weekStart + 7) % 7));
    const end = new Date(start);
    end.setDate(end.getDate() + 6);
    return `${start.getMonth() + 1}月${start.getDate()}日 – ${end.getMonth() + 1}月${end.getDate()}日`;
  }

  function fmtDateStr(y: number, m: number, d: number): string {
    const dt = new Date(y, m, d);
    return `${dt.getFullYear()}-${String(dt.getMonth() + 1).padStart(2, "0")}-${String(dt.getDate()).padStart(2, "0")}`;
  }

  function calPrev(): void { if (calMonth === 0) { calYear--; calMonth = 11; } else calMonth--; }
  function calNext(): void { if (calMonth === 11) { calYear++; calMonth = 0; } else calMonth++; }

  function collapsedLine(md: string): string {
    const firstLine = md.split("\n")[0] ?? "";
    return firstLine.replace(/^#+\s*/, "");
  }

  /** 「已完成」固定按完成时间降序：最新完成的在最上面（与列表排序方式无关）。 */
  function byCompletedDesc(tasks: Task[]): Task[] {
    return [...tasks].sort((a, b) =>
      (b.completedAt ?? b.createdAt).localeCompare(a.completedAt ?? a.createdAt)
    );
  }

  /** 这一页有没有浮层开着（链接预览 / 建议 / 日历 / 分组面板 / 三个菜单）。 */
  $: workspaceOverlayOpen =
    showSuggestions ||
    showCalendar ||
    showHeaderMenu ||
    showPlannedGroups ||
    linkPreviewUrl !== "" ||
    taskMenu !== null ||
    diaryMenu !== null ||
    listMenuAt !== null;

  // 返回键先收掉本页浮层（与「点外面」同一个动作），再往下才轮到历史栈。
  const backGuard = createBackGuard();
  $: backGuard(workspaceOverlayOpen, closeOverlays);

  export function closeOverlays(): void {
    showSuggestions = false;
    showCalendar = false;
    // 日历的年月浮层也要收：v0.8.2 给 DiaryView / LedgerView 都补了这一句，
    // 漏了 Workspace 这第三处——开着年月浮层时经别的路径关掉日历（点页面外、切视图），
    // 下次打开日历浮层会「不请自来」
    calPopOpen = false;
    showHeaderMenu = false;
    showPlannedGroups = false;
    schedulerViewRef?.closeOverlays();
    taskMenu = null;
    diaryMenu = null;
    listMenuAt = null;
    linkPreviewUrl = "";
    linkPreviewTitle = "";
    taskEmojiPicker.set(null);
  }

  export function focusComposer(): void {
    taskInput?.focus();
  }

  function toggleCompletion(taskId: string): void {
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (task) {
      void updateTaskAction(taskId, { completed: !task.completed });
    }
  }

  function addToMyDay(taskId: string): void {
    void updateTaskAction(taskId, { myDay: true });
  }

  /**
   * 「日期与提醒」面板保存/清除：写操作住在 `actions.setTaskSchedule`
   * （卡片浮层与搜索结果两个入口共用同一份语义），这里只负责收菜单。
   */
  function applyTaskSchedule(
    taskId: string,
    patch: { dueDate: string; dueTime: string; reminders: ReminderRule[] }
  ): void {
    taskMenu = null;
    void setTaskScheduleAction(taskId, patch);
  }

  function handleTaskSetSchedule(
    event: CustomEvent<{ id: string; dueDate: string; dueTime: string; reminders: ReminderRule[] }>
  ): void {
    const { id, ...patch } = event.detail;
    applyTaskSchedule(id, patch);
  }

  function toggleSuggestions(): void {
    showSuggestions = !showSuggestions;
    showCalendar = false;
    showHeaderMenu = false;
    listMenuAt = null;
    taskMenu = null;
  }

  function toggleCalendar(): void {
    showCalendar = !showCalendar;
    showSuggestions = false;
    showHeaderMenu = false;
    listMenuAt = null;
    taskMenu = null;
    if (showCalendar) {
      const d = new Date();
      calYear = d.getFullYear();
      calMonth = d.getMonth();
    }
  }

  /** 移动端头部齿轮：开面板时收起其它头部浮层。 */
  function toggleHeaderMenu(): void {
    showHeaderMenu = !showHeaderMenu;
    if (showHeaderMenu) {
      showSuggestions = false;
      showCalendar = false;
      listMenuAt = null;
      taskMenu = null;
    }
  }

  /** 齿轮面板 → 列表菜单：锚在齿轮按钮右下角（视口像素，ContextMenu 内部除以缩放）。 */
  function openListMenuFromGear(): void {
    showHeaderMenu = false;
    const rect = gearButtonEl?.getBoundingClientRect();
    if (!rect) return;
    listMenuFromButton = false;
    listMenuAt = { x: rect.right, y: rect.bottom + 6 };
    showSuggestions = false;
    showCalendar = false;
    taskMenu = null;
  }

  /** 展开全部 / 收起全部（两个独立动作，非 toggle）。
   *  可展开的判据是**卡片自己量出来的**（多行 ∪ 单行超长要折行），不是光看 markdown 行数。 */
  function expandAll(expanded: boolean): void {
    const ids = $visibleTasks.filter((task) => !expanded || isExpandable(task)).map((task) => task.id);
    if (ids.length > 0) {
      void setItemsUiAction(ids, expanded);
    }
  }

  /** 卡片量出来的可展开性（单行超长的那批）——由 TaskCard 的 measure 事件维护。
   *  只装「报了 true」的 id：缺席就是 false，于是绝大多数卡片的首次上报（false）
   *  根本不必惊动响应式系统。陈旧 id（任务已删）留着无害——它永远匹配不上可见任务。 */
  let measuredExpandable = new Set<string>();

  function handleCardMeasure(event: CustomEvent<{ id: string; canExpand: boolean }>): void {
    const { id, canExpand } = event.detail;
    if (canExpand === measuredExpandable.has(id)) return;
    if (canExpand) measuredExpandable.add(id);
    else measuredExpandable.delete(id);
    // 自赋值触发失效：Svelte 只看「这个变量被赋值过」，不比较内容。
    // 早先这里是 `new Map(measuredExpandable)` 整表拷贝，而每张卡首次上报时 `get(id)`
    // 是 undefined、必然与 canExpand 不等 → N 张卡挂载 = N 次递增规模的拷贝（O(N²)）。
    measuredExpandable = measuredExpandable;
  }

  /** 这张卡片有没有可展开的内容：多行，或者卡片量出来「折叠态显示不全」。 */
  function isExpandable(task: Task): boolean {
    return hasMultipleMarkdownLines(task.markdown) || measuredExpandable.has(task.id);
  }

  /** 展开/收起一张卡片。`expanded` 由卡片自己量出来（单行但显示不全也算可展开）；
   * 没给时退回多行规则——「展开全部/收起全部」走这条路。 */
  function toggleTaskExpansion(taskId: string, expanded?: boolean): void {
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (!task) return;
    const next = expanded ?? (isExpandable(task) ? !task.expanded : false);
    void setItemUiAction(taskId, { expanded: next });
  }

  function openTaskEditor(taskId: string): void {
    taskMenu = null;
    editorTaskId.set(taskId);
  }

  function deleteTask(taskId: string): void {
    void deleteTaskAction(taskId);
    taskMenu = null;
  }

  function addTagToTask(
    taskId: string,
    tag: { color: TagColor; hex?: string; text?: string }
  ): void {
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (!task) return;
    void replaceTaskTagsAction(taskId, [
      ...task.tags,
      {
        id: `tag-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`,
        color: tag.color,
        text: tag.text?.trim() || undefined,
        hex: tag.color === "custom" ? tag.hex : undefined
      }
    ]);
  }

  function removeTagFromTask(taskId: string, tagId: string): void {
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (task) {
      void replaceTaskTagsAction(taskId, task.tags.filter((t) => t.id !== tagId));
    }
  }

  function editTagAtTask(taskId: string, tagId: string, text: string): void {
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (task) {
      void replaceTaskTagsAction(taskId, task.tags.map((t) => (t.id === tagId ? { ...t, text: text || undefined } : t)));
    }
  }

  function moveTaskToNode(taskId: string, targetNodeId: string): void {
    void updateTaskAction(taskId, { entryId: targetNodeId });
    taskMenu = null;
  }

  function taskTargetNode(): AppNode | undefined {
    return $selectedNode?.kind === "entry" ? $selectedNode : $appState.nodes.find((n) => n.kind === "entry");
  }

  function addTaskFromDraft(): void {
    const markdown = newTaskDraft.trim();
    if (!markdown) return;
    const targetNode = taskTargetNode();
    if (!targetNode) {
      showToast("请先创建一个条目");
      return;
    }
    void addTaskAction(targetNode.id, {
      markdown,
      important: $selectedNode?.id === "important",
      myDay: $selectedNode?.id === "my-day"
    });
    newTaskDraft = "";
    void tick().then(resizeComposer);
  }

  /** 加号：直接开编辑器新建一条（归属条目与输入框同一个）。 */
  function openComposerEditor(): void {
    const targetNode = taskTargetNode();
    if (!targetNode) {
      showToast("请先创建一个条目");
      return;
    }
    editorDraftNode.set(targetNode.id);
  }

  function handleComposerKeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      addTaskFromDraft();
    }
  }

  async function handleComposerPaste(event: ClipboardEvent): Promise<void> {
    if (!isTauriRuntime) return;
    const items = event.clipboardData?.items;
    if (!items) return;
    for (const item of items) {
      if (!item.type.startsWith("image/")) continue;
      event.preventDefault();
      const targetNode = taskTargetNode();
      if (!targetNode) {
        showToast("请先创建一个条目");
        return;
      }
      const file = item.getAsFile();
      if (!file) return;
      try {
        const dataUrl = await fileToDataUrl(file);
        const filename = await saveMdImageFromDataUrl(dataUrl, targetNode.id);
        const url = await mdImageUrl(targetNode.id, filename);
        primeMdImageCache(targetNode.id, filename, url);
        const cursorStart = taskInput?.selectionStart ?? newTaskDraft.length;
        const cursorEnd = taskInput?.selectionEnd ?? cursorStart;
        const before = newTaskDraft.slice(0, cursorStart);
        const after = newTaskDraft.slice(cursorEnd);
        newTaskDraft = `${before}\n![](${filename})\n${after}`;
        await tick();
        resizeComposer();
      } catch (error) {
        showToast(`图片粘贴失败：${String(error)}`);
      }
      return;
    }
  }

  function resizeComposer(): void {
    if (!taskInput) return;
    taskInput.style.height = "auto";
    taskInput.style.height = `${Math.min(taskInput.scrollHeight, 180)}px`;
  }

  function openTaskMenu(event: CustomEvent<{ id: string; x: number; y: number }>): void {
    taskMenu = { taskId: event.detail.id, x: event.detail.x, y: event.detail.y };
    diaryMenu = null;
    listMenuAt = null;
  }

  // ---- 搜索结果里的日记卡片 ----
  function openDiaryMenu(event: CustomEvent<{ id: string; x: number; y: number }>): void {
    diaryMenu = { id: event.detail.id, x: event.detail.x, y: event.detail.y };
    taskMenu = null;
    listMenuAt = null;
  }

  function openDiaryEntry(id: string): void {
    diaryMenu = null;
    diaryEditor.set({ id });
  }

  // ---- 搜索结果里的记账卡片 ----
  /** 记账结果卡的点击：打开记账面板改这一笔（面板挂在 App 层，这里只递 id）。 */
  function openLedgerEntry(id: string): void {
    diaryMenu = null;
    taskMenu = null;
    ledgerEditor.set({ id });
  }

  function handleDiaryExpand(event: CustomEvent<{ id: string; expanded: boolean }>): void {
    void setDiaryUiAction(event.detail.id, { expanded: event.detail.expanded });
  }

  /** 桌面三点按钮：**支持 toggle**——菜单已经开着（且是它开的）时再点一次就收起。
   *  ContextMenu 拿到了这个按钮作 anchor，点它身上不会被「点外面」抢先关掉，
   *  所以这里的判断看到的是真实状态（不是刚被关掉的空值）。 */
  function openListMenu(event: MouseEvent): void {
    if (listMenuAt && listMenuFromButton) {
      listMenuAt = null;
      return;
    }
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    listMenuFromButton = true;
    listMenuAt = { x: rect.right, y: rect.bottom + 6 };
    showSuggestions = false;
    showCalendar = false;
    taskMenu = null;
  }

  function beginHeaderRename(): void {
    if (!$selectedNode || $selectedNode.kind === "system") return;
    listMenuAt = null;
    headerRenaming = true;
    headerRenameDraft = $selectedNode.name;
    void tick().then(() => {
      headerRenameInput?.focus();
      headerRenameInput?.select();
    });
  }

  function commitHeaderRename(): void {
    if (!headerRenaming) return;
    const name = headerRenameDraft.trim();
    headerRenaming = false;
    if (!name || !$selectedNode) return;
    void renameNodeAction($selectedNode.id, name);
  }

  function handleHeaderRenameKeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Enter") {
      event.preventDefault();
      commitHeaderRename();
    } else if (event.key === "Escape") {
      headerRenaming = false;
    }
  }

  /** 供 App（编辑器浮窗）与 TaskCard 复用的链接打开入口。 */
  export function openLinkUrl(url: string, title?: string): void {
    if ($appSettings.appearance.linkOpenMode === "system") {
      void openExternalUrl(url).catch((error) => showToast(`打开链接失败：${String(error)}`));
    } else {
      linkPreviewUrl = url;
      linkPreviewTitle = (title ?? "").trim() || hostOf(url);
    }
  }

  /** 标题兜底：拿不到链接文字就显示主机名（总比一串 URL 好读）。 */
  function hostOf(url: string): string {
    try {
      return new URL(url).hostname;
    } catch {
      return url;
    }
  }

  function openTaskLink(event: CustomEvent<{ href: string; title: string }>): void {
    openLinkUrl(event.detail.href, event.detail.title);
  }

  /** 同源时能读到网页真正的标题（跨源会抛 SecurityError，忽略，保留链接文字/主机名）。 */
  function readPreviewTitle(): void {
    try {
      const title = previewFrame?.contentDocument?.title?.trim();
      if (title) linkPreviewTitle = title;
    } catch {
      // 跨源：读不到就用已有的标题
    }
  }

  function closeLinkPreview(): void {
    linkPreviewUrl = "";
    linkPreviewTitle = "";
  }

  /** 网站拒绝被 iframe 框住时（X-Frame-Options / CSP frame-ancestors）的一条出路。 */
  function openPreviewExternal(): void {
    const url = linkPreviewUrl;
    if (!url) return;
    void openExternalUrl(url).catch((error) => showToast(`打开链接失败：${String(error)}`));
  }

  function openEmojiPickerForTask(taskId: string): void {
    taskEmojiPicker.set({ taskId, index: -1 });
    taskMenu = null;
  }

  function openEmojiPickerAt(taskId: string, index: number): void {
    taskEmojiPicker.set({ taskId, index });
  }

  function removeEmojiFromTask(taskId: string, index: number): void {
    const task = $appState.tasks.find((item) => item.id === taskId);
    if (task) {
      void replaceTaskEmojisAction(taskId, task.emojis.filter((_, i) => i !== index));
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<svelte:window on:keydown={handlePanelKeydown} />

<main class="workspace" style={mainStyle} inert={coveredByFullPage}>
  <section class="list-header">
    <div>
      <MobileBack />
      <span class="header-icon">
        {#if $isSearching}
          <Search size={PAGE_HEADER_ICON_SIZE} />
        {:else}
          <IconGlyph icon={$selectedNode?.icon ?? "notebook"} size={PAGE_HEADER_ICON_SIZE} />
        {/if}
      </span>
      {#if headerRenaming}
        <input
          bind:this={headerRenameInput}
          bind:value={headerRenameDraft}
          class="header-rename-input"
          maxlength="60"
          on:blur={commitHeaderRename}
          on:keydown={handleHeaderRenameKeydown}
          on:click|stopPropagation
        />
      {:else}
        <h1>{$isSearching ? `搜索结果：${$searchQuery}` : $selectedNode?.name ?? "KXToDo"}</h1>
        {#if $isSearching && $searchScanning}
          <!-- 扫描在后台分片跑，结果还没定稿：给个明示，别让人以为「就这几条」 -->
          <span class="search-scanning">搜索中…</span>
        {/if}
      {/if}
    </div>
    <div class="header-actions" on:click|stopPropagation>
      {#if $isMobile}
        <!-- 移动端：单一齿轮按钮 + 下拉面板（替代旧版“点标题显示动作”的机制） -->
        <button
          bind:this={gearButtonEl}
          type="button"
          title="更多操作"
          aria-label="更多操作"
          aria-expanded={showHeaderMenu}
          on:click|stopPropagation={toggleHeaderMenu}
        ><SettingsIcon size={21} /></button>
        {#if showHeaderMenu}
          <div class="header-menu-panel" role="menu" tabindex="-1">
            {#if isMyDay}
              <MenuItem icon={Lightbulb} label="建议添加" onSelect={toggleSuggestions} />
              <MenuItem icon={Calendar} label="完成日历" onSelect={toggleCalendar} />
            {/if}
            {#if !allExpanded}
              <MenuItem icon={ChevronsDown} label="展开全部" onSelect={() => { showHeaderMenu = false; expandAll(true); }} />
            {/if}
            {#if !allCollapsed}
              <MenuItem icon={ChevronsUp} label="收起全部" onSelect={() => { showHeaderMenu = false; expandAll(false); }} />
            {/if}
            {#if syncAvailable}
              <MenuItem icon={RefreshCw} label={pullBusy ? "同步中…" : "立即同步"} onSelect={() => { showHeaderMenu = false; void runManualSyncNow(); }} />
            {/if}
            <MenuItem icon={MoreHorizontal} label="列表菜单" onSelect={openListMenuFromGear} />
          </div>
        {/if}
      {:else}
        {#if isScheduled}
          <button type="button" title="执行器路径" on:click|stopPropagation={() => schedulerViewRef?.toggleRuntimeSettings()}>
            <SettingsIcon size={21} />
          </button>
        {/if}
        {#if !isScheduled}
          {#if isMyDay}
            <button type="button" title="完成日历" on:click|stopPropagation={toggleCalendar}>
              <Calendar size={21} />
            </button>
            <button type="button" title="建议添加" on:click|stopPropagation={toggleSuggestions}>
              <Lightbulb size={21} />
            </button>
          {/if}
          {#if !allCollapsed}
            <button
              type="button"
              title="收起全部"
              on:click|stopPropagation={() => expandAll(false)}
            ><ChevronsUp size={21} /></button>
          {/if}
          {#if !allExpanded}
            <button
              type="button"
              title="展开全部"
              on:click|stopPropagation={() => expandAll(true)}
            ><ChevronsDown size={21} /></button>
          {/if}
        {/if}

        <button
          bind:this={listMenuButtonEl}
          type="button"
          title="列表菜单"
          on:mousedown|preventDefault|stopPropagation={openListMenu}
          on:click|stopPropagation
        ><MoreHorizontal size={23} /></button>
      {/if}

      {#if showSuggestions && !isScheduled}
      <section class="suggestion-panel" on:click|stopPropagation>
        <div class="suggestion-panel-title">建议添加到我的一天</div>
        {#if suggestedTasks.length === 0}
          <div class="suggestion-empty">暂无建议</div>
        {:else}
          {#each suggestedTasks as task (task.id)}
            <div class="suggestion-item">
              <span class="suggestion-text">{collapsedLine(task.markdown)}</span>
              <button class="suggestion-add" type="button" title="添加到我的一天" on:click|stopPropagation={() => addToMyDay(task.id)}>
                <Plus size={16} />
              </button>
            </div>
          {/each}
        {/if}
      </section>
      {/if}

      {#if showCalendar && !isScheduled}
      <section class="calendar-panel" on:click|stopPropagation>
        <div class="calendar-header">
          <button type="button" on:click={calPrev}><ChevronLeft size={16} /></button>
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_noninteractive_element_to_interactive_role -->
          <span
            bind:this={calLabelEl}
            class="month-pop-anchor cal-title"
            role="button"
            tabindex="0"
            title="点击直接选年月"
            on:click={() => (calPopOpen = !calPopOpen)}
          >{calYear}年{calMonth + 1}月</span>
          <button type="button" on:click={calNext}><ChevronRight size={16} /></button>
        </div>
        <MonthPopover
          open={calPopOpen}
          anchor={calLabelEl}
          year={calYear}
          month={calMonth}
          onSelect={(next) => {
            calYear = next.year;
            calMonth = next.month;
          }}
          onClose={() => (calPopOpen = false)}
        />
        <div class="calendar-view-toggle">
          <button type="button" class:active={calViewMode === "month"} on:click={() => (calViewMode = "month")}>月</button>
          <button type="button" class:active={calViewMode === "week"} on:click={() => (calViewMode = "week")}>周</button>
        </div>
        {#if calViewMode === "month"}
          <div class="calendar-grid">
            {#each weekDayLabels as label}
              <span class="day-header">{label}</span>
            {/each}
            {#each calMonthDays() as cell}
              <button
                type="button"
                class="day-cell"
                class:other-month={!cell.current}
                class:today={cell.date === todayIso()}
                class:selected={cell.date === myDayViewDate}
                class:has-tasks={cell.hasTask}
                on:click={() => (myDayViewDate = cell.date)}
              >{cell.day}</button>
            {/each}
          </div>
          <div class="calendar-tasks-title">
            {(() => { const p = myDayViewDate.split("-").map(Number); return `${p[1]}月${p[2]}日 完成的任务`; })()}
          </div>
          {#if calSelectedTasks.length === 0}
            <div class="calendar-no-tasks">无完成任务</div>
          {:else}
            {#each calSelectedTasks as task (task.id)}
              <div class="calendar-task-item">{collapsedLine(task.markdown)}</div>
            {/each}
          {/if}
        {:else}
          <div class="calendar-tasks-title">
            {calWeekRange()} · 共 {weekSummaryTotal} 项
          </div>
          <div class="week-summary">
            {#each weekSummary as day (day.date)}
              <div class="week-summary-day" class:active={day.date === myDayViewDate}>
                <button type="button" class="week-summary-head" class:is-today={day.date === todayIso()} on:click={() => (myDayViewDate = day.date)}>
                  <span>{day.label}</span>
                  <span class="week-summary-count">{day.tasks.length}</span>
                </button>
                {#each day.tasks as task (task.id)}
                  <div class="calendar-task-item">{collapsedLine(task.markdown)}</div>
                {/each}
              </div>
            {/each}
          </div>
        {/if}
      </section>
      {/if}
    </div>
  </section>

  {#if listMenuAt}
    <ListMenu
      x={listMenuAt.x}
      y={listMenuAt.y}
      xAlign="right"
      anchor={listMenuFromButton ? listMenuButtonEl : null}
      node={$selectedNode}
      {isScheduled}
      {isPlanned}
      showCompleted={plannedShowCompleted}
      onToggleShowCompleted={togglePlannedCompleted}
      {sortMode}
      onSortMode={(mode) => (sortMode = mode)}
      onRenameRequest={beginHeaderRename}
      onClose={() => (listMenuAt = null)}
    />
  {/if}

  {#if isScheduled}
    <ScheduledTasksView bind:this={schedulerViewRef} />
  {:else}
  {#if isMyDay}
    <p class="list-subtitle">
      {formatMyDayDate(myDayViewDate)}
      {#if isMyDayHistory}
        <button type="button" class="my-day-back" on:click={() => (myDayViewDate = todayIso())}>返回今天</button>
      {/if}
    </p>
  {:else if isPlanned}
    <div class="planned-group-bar">
      <button
        class="planned-group-chip"
        type="button"
        aria-expanded={showPlannedGroups}
        on:click|stopPropagation={() => (showPlannedGroups = !showPlannedGroups)}
      >
        {plannedGroupLabel}
        <ChevronDown size={15} />
      </button>
      {#if showPlannedGroups}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div class="planned-group-panel" role="menu" tabindex="-1" on:click|stopPropagation>
          {#each plannedOptions as option (option.key)}
            <MenuItem
              label={option.label}
              checkable
              active={option.key === plannedGroup}
              onSelect={() => { plannedGroup = option.key; showPlannedGroups = false; }}
            />
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  <section
    class="task-list"
    bind:this={listScrollEl}
    use:pullToRefresh={{
      enabled: () => syncAvailable && $mobileView === "content",
      busy: () => pullBusy,
      onProgress: (distance, ready) => {
        pullDistance = distance;
        pullReady = ready;
      },
      onRelease: () => void runManualSyncNow()
    }}
  >
    {#if $isMobile && syncAvailable && (pullDistance > 0 || pullBusy)}
      <div class="pull-sync-zone" style={`height:${pullBusy ? 58 : Math.max(32, Math.round(pullDistance * 0.7))}px`}>
        <span
          class="pull-sync-indicator"
          class:ready={pullReady || pullBusy}
          class:spinning={pullBusy}
          style={`transform: rotate(${Math.min(360, Math.round((pullDistance / 72) * 180))}deg)`}
        >
          <RefreshCw size={17} />
        </span>
      </div>
    {/if}
    {#if $isSearching}
      <!-- 全局搜索：任务卡（todo / 一般）、日记卡与记账卡按「最近改动」混排在一条列表里。
           结果走 VirtualStack（v0.8.6 需求 1）：命中封顶 200 条、只有视口附近的行在树上。 -->
      <VirtualStack
        items={$searchHits}
        keyOf={(hit) => (hit as SearchHit).key}
        scroller={listScrollEl}
        estimate={84}
        overscan={10}
      >
        <svelte:fragment slot="item" let:row>
          {@const hit = row as SearchHit}
          {#if hit.kind === "task"}
            <TaskCard
              task={hit.task}
              nodeId={hit.task.nodeId}
              cardStyle={hit.cardStyle}
              selected={taskMenu?.taskId === hit.task.id}
              on:toggle={(event) => toggleCompletion(event.detail)}
              on:expand={(event) => toggleTaskExpansion(event.detail.id, event.detail.expanded)}
              on:measure={handleCardMeasure}
              on:edit={(event) => openTaskEditor(event.detail)}
              on:context={openTaskMenu}
              on:openLink={openTaskLink}
              on:setSchedule={handleTaskSetSchedule}
              on:removeTag={(e) => removeTagFromTask(e.detail.id, e.detail.tagId)}
              on:editTag={(e) => editTagAtTask(e.detail.id, e.detail.tagId, e.detail.text)}
              on:removeEmoji={(e) => removeEmojiFromTask(e.detail.id, e.detail.index)}
              on:pickEmoji={(e) => openEmojiPickerAt(e.detail.id, e.detail.index)}
            />
          {:else if hit.kind === "diary"}
            <DiaryCard
              entry={hit.entry}
              today={todayIso()}
              selected={diaryMenu?.id === hit.entry.id}
              on:expand={handleDiaryExpand}
              on:edit={(event) => openDiaryEntry(event.detail)}
              on:context={openDiaryMenu}
              on:openLink={openTaskLink}
            />
          {:else}
            <!-- 记账结果：按日期归属条目的主题色画（记账页自己的 accent） -->
            <div class="search-hit" style={`--accent: ${ledgerAccentColor}`}>
              <LedgerEntryCard
                book={$ledgerData}
                entry={hit.entry}
                on:edit={(event) => openLedgerEntry(event.detail)}
              />
            </div>
          {/if}
        </svelte:fragment>
      </VirtualStack>
    {:else}
    <!-- 任务一多（>100）就窗口化（v0.8.4 需求 5）：只有视口附近那几十张卡在树上，
         公式与日记列表同一套（VirtualStack / windowing.ts）。100 以内全量直出，
         不为小列表付虚拟化的代价。 -->
    <VirtualStack
      items={taskRows}
      keyOf={(row) => (row as TaskRow).key}
      scroller={listScrollEl}
      fullBelow={100}
      estimate={84}
      overscan={10}
    >
      <svelte:fragment slot="item" let:row>
        {@const item = row as TaskRow}
        {#if item.kind === "label"}
          <button class="task-section-label" type="button" on:click|stopPropagation={() => toggleSection(item.sectionKey)}>
            <ChevronDown class={item.collapsed ? "collapsed" : ""} size={15} />
            {item.label} {item.count}
          </button>
        {:else if item.task}
          <TaskCard
            task={item.task}
            nodeId={item.task.nodeId}
            cardStyle={cardStyleByNode.get(item.task.nodeId) ?? "todo"}
            selected={taskMenu?.taskId === item.task.id}
            on:toggle={(event) => toggleCompletion(event.detail)}
            on:expand={(event) => toggleTaskExpansion(event.detail.id, event.detail.expanded)}
            on:measure={handleCardMeasure}
            on:edit={(event) => openTaskEditor(event.detail)}
            on:context={openTaskMenu}
            on:openLink={openTaskLink}
            on:setSchedule={handleTaskSetSchedule}
            on:removeTag={(e) => removeTagFromTask(e.detail.id, e.detail.tagId)}
            on:editTag={(e) => editTagAtTask(e.detail.id, e.detail.tagId, e.detail.text)}
            on:removeEmoji={(e) => removeEmojiFromTask(e.detail.id, e.detail.index)}
            on:pickEmoji={(e) => openEmojiPickerAt(e.detail.id, e.detail.index)}
          />
        {/if}
      </svelte:fragment>
    </VirtualStack>

    {#if !$isSearching && completedTasks.length}
      <section class="completed-section">
        <button class="completed-toggle" type="button" on:click|stopPropagation={toggleCompletedSection}>
          <ChevronDown class={!showCompleted ? "collapsed" : ""} size={17} />
          已完成 {completedTasks.length}
        </button>
        {#if showCompleted}
          {#each completedTasks as task (task.id)}
            <TaskCard
              {task}
              nodeId={task.nodeId}
              cardStyle={cardStyleByNode.get(task.nodeId) ?? "todo"}
              selected={taskMenu?.taskId === task.id}
              on:toggle={(event) => toggleCompletion(event.detail)}
              on:expand={(event) => toggleTaskExpansion(event.detail.id, event.detail.expanded)}
              on:measure={handleCardMeasure}
              on:edit={(event) => openTaskEditor(event.detail)}
              on:context={openTaskMenu}
              on:openLink={openTaskLink}
              on:setSchedule={handleTaskSetSchedule}
              on:removeTag={(e) => removeTagFromTask(e.detail.id, e.detail.tagId)}
              on:editTag={(e) => editTagAtTask(e.detail.id, e.detail.tagId, e.detail.text)}
              on:removeEmoji={(e) => removeEmojiFromTask(e.detail.id, e.detail.index)}
              on:pickEmoji={(e) => openEmojiPickerAt(e.detail.id, e.detail.index)}
            />
          {/each}
        {/if}
      </section>
    {/if}
    {/if}

    {#if $isSearching ? $searchHits.length === 0 : incompleteTasks.length === 0 && completedTasks.length === 0}
      <div class="empty-state">
        {#if $isSearching && $searchScanning}
          <strong class="search-progress">搜索中…</strong>
        {:else}
        <strong>{$isSearching
          ? "没有搜索结果"
          : isMyDayHistory
            ? "这一天没有完成任何事项"
            : isPlanned && plannedGroup !== "all"
              ? "该分组下暂无任务"
              : "这个条目还没有内容"}</strong>
        {#if !isMyDayHistory && !(isPlanned && plannedGroup !== "all")}
          <span>在下方输入 Markdown，按 Enter 添加；Shift + Enter 换行。</span>
        {/if}
        {/if}
      </div>
    {/if}
  </section>

  {#if diaryMenu && diaryMenuEntry}
    <DiaryEntryMenu
      x={diaryMenu.x}
      y={diaryMenu.y}
      entry={diaryMenuEntry}
      today={todayIso()}
      on:edit={(event) => openDiaryEntry(event.detail)}
      on:close={() => (diaryMenu = null)}
    />
  {/if}

  {#if taskMenu && taskMenuTask}
    <ContextMenu x={taskMenu.x} y={taskMenu.y} minWidth={236} onClose={() => (taskMenu = null)}>
      <MenuItem
        icon={Sun}
        label={taskMenuTask.myDay ? "从我的一天中移除" : "添加到我的一天"}
        onSelect={() => { void updateTaskAction(taskMenuTask.id, { myDay: !taskMenuTask.myDay }); taskMenu = null; }}
      />
      <MenuItem icon={CalendarDays} label="日期与提醒">
        <div slot="submenu" class="task-menu-date">
          <TaskDateReminderPanel
            embedded
            dueDate={taskMenuTask.dueDate?.slice(0, 10) ?? ""}
            dueTime={taskMenuTask.dueTime ?? ""}
            reminders={taskMenuTask.reminders ?? []}
            onSave={(patch) => applyTaskSchedule(taskMenuTask.id, patch)}
            onClear={() => applyTaskSchedule(taskMenuTask.id, { dueDate: "", dueTime: "", reminders: [] })}
            onClose={() => (taskMenu = null)}
          />
        </div>
      </MenuItem>
      <MenuItem
        icon={Star}
        label={taskMenuTask.important ? "取消收藏" : "收藏"}
        onSelect={() => { void updateTaskAction(taskMenuTask.id, { important: !taskMenuTask.important }); taskMenu = null; }}
      />
      <MenuItem icon={Tag} label="标签">
        <div slot="submenu" class="tag-editor-panel" on:click|stopPropagation>
          <TagMenuPanel onAdd={(tag) => addTagToTask(taskMenuTask.id, tag)} />
        </div>
      </MenuItem>
      <MenuItem icon={SmilePlus} label="添加表情" onSelect={() => openEmojiPickerForTask(taskMenuTask.id)} />
      <MenuItem icon={FolderInput} label="移动到">
        <div slot="submenu" class="submenu-list">
          <MoveTargetTree
            nodes={$appState.nodes}
            currentEntryId={taskMenuTask.nodeId}
            on:move={(event) => moveTaskToNode(taskMenuTask.id, event.detail)}
          />
          {#if !hasTaskMoveTargets}
            <div class="menu-empty">没有可移动的目标</div>
          {/if}
        </div>
      </MenuItem>
      <MenuSeparator />
      <MenuItem icon={PenLine} label="编辑" onSelect={() => openTaskEditor(taskMenuTask.id)} />
      <MenuItem icon={Trash2} danger label="删除" onSelect={() => deleteTask(taskMenuTask.id)} />
    </ContextMenu>
  {/if}

  {#if !isMyDayHistory}
    <section class="add-task-bar" on:click|stopPropagation>
      <button class="composer-plus" type="button" title="用编辑器新建事项" aria-label="用编辑器新建事项" on:click|stopPropagation={openComposerEditor}>
        <Plus size={24} />
      </button>
      <div class="composer-main">
        <textarea
          bind:this={taskInput}
          bind:value={newTaskDraft}
          placeholder="添加事项"
          spellcheck="false"
          rows="1"
          on:input={resizeComposer}
          on:keydown={handleComposerKeydown}
          on:paste={handleComposerPaste}
        ></textarea>
      </div>
    </section>
  {/if}
  {/if}

  {#if linkPreviewUrl}
    <div class="link-preview-overlay">
      <div class="link-preview-bar">
        <span class="link-preview-title" title={linkPreviewTitle}>{linkPreviewTitle}</span>
        <button
          class="link-preview-close"
          type="button"
          title="在系统浏览器打开"
          aria-label="在系统浏览器打开"
          on:click={openPreviewExternal}
        >
          <ExternalLink size={17} />
        </button>
        <button class="link-preview-close" type="button" title="关闭预览" aria-label="关闭预览" on:click={closeLinkPreview}>
          <X size={18} strokeWidth={2.5} />
        </button>
      </div>
      <iframe
        bind:this={previewFrame}
        class="link-preview-frame"
        src={linkPreviewUrl}
        title="链接预览"
        sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
        on:load={readPreviewTitle}
      ></iframe>
    </div>
  {/if}

</main>
