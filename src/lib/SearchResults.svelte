<script lang="ts">
  import { ExternalLink, NotebookPen, PenLine, Pin, PinOff, Trash2 } from "@lucide/svelte";
  import {
    appState, appSettings, diaryEditor, editorTaskId, ledgerData, ledgerEditor, searchHits, searchQuery, searchScanning,
    showToast,
    taskEmojiPicker, todayIso
  } from "./stores";
  import {
    deleteDiaryEntry, deleteTask as deleteTaskAction, replaceTaskEmojis as replaceTaskEmojisAction,
    replaceTaskTags as replaceTaskTagsAction, selectNode as selectNodeAction,
    setDiaryUi as setDiaryUiAction, setItemUi as setItemUiAction,
    updateTask as updateTaskAction, setTaskSchedule as setTaskScheduleAction
  } from "./actions";
  import { openLink as openLinkWithPreference } from "./backend";
  import { showMobileContent, showMobileDiary } from "./platform";
  import { accentForNode, diaryAccent, ledgerAccent } from "./styles";
  import TaskCard from "./TaskCard.svelte";
  import VirtualStack from "./VirtualStack.svelte";
  import DiaryCard from "./diary/DiaryCard.svelte";
  import DiaryEntryMenu from "./diary/DiaryEntryMenu.svelte";
  import LedgerEntryCard from "./ledger/LedgerEntryCard.svelte";
  import ContextMenu from "./menu/ContextMenu.svelte";
  import MenuItem from "./menu/MenuItem.svelte";
  import MenuSeparator from "./menu/MenuSeparator.svelte";
  import type { ReminderRule, SearchHit, Tag, TagColor } from "./types";

  /**
   * 移动端全局搜索的结果面板：挂在侧栏搜索框下面，占大半屏。
   *
   * 为什么要有它——搜索结果本来渲染在工作区里，而移动端首屏是列表视图，
   * `.app-shell.mobile.view-list .workspace` 是 `display:none`，于是安卓上搜什么都「没有反应」。
   * 卡片与桌面完全同一套组件（TaskCard / DiaryCard），所以三种卡片的混排是白拿的；
   * 卡片上的每个可点控件都在这里接了线，不留按了没反应的死控件。
   */
  let taskMenu: { id: string; nodeId: string; x: number; y: number } | null = null;
  let diaryMenu: { id: string; x: number; y: number } | null = null;
  /** 结果面板自己就是滚动容器（`.search-results` 是 overflow-y:auto），VirtualStack 要它 */
  let resultsEl: HTMLElement | null = null;

  $: diaryHit = diaryMenu
    ? $searchHits.find((hit) => hit.kind === "diary" && hit.entry.id === diaryMenu?.id)
    : null;
  $: diaryEntry = diaryHit && diaryHit.kind === "diary" ? diaryHit.entry : null;
  $: taskMenuTask = taskMenu ? $appState.tasks.find((task) => task.id === taskMenu?.id) : null;

  /** 结果面板挂在侧栏里，拿不到工作区内联的 --accent：勾选圆圈、日期栏这些靠
      var(--accent) 画的控件会整个消失。每条结果自带所属条目的主题色。 */
  function hitAccent(nodeId: string): string {
    return accentForNode($appState.nodes.find((node) => node.id === nodeId), $appSettings.appearance.uiColors);
  }

  $: diaryHitAccent = diaryAccent($appSettings.diary);
  $: ledgerAccentColor = ledgerAccent($appSettings.ledger);

  function closeMenus(): void {
    taskMenu = null;
    diaryMenu = null;
  }

  function findTask(id: string) {
    return $appState.tasks.find((task) => task.id === id);
  }

  // ---- 任务卡片 ----
  function handleTaskExpand(event: CustomEvent<{ id: string; expanded: boolean }>): void {
    void setItemUiAction(event.detail.id, { expanded: event.detail.expanded });
  }

  function toggleTask(id: string): void {
    const task = findTask(id);
    if (task) void updateTaskAction(id, { completed: !task.completed });
  }

  function openTaskEditor(id: string): void {
    closeMenus();
    editorTaskId.set(id);
  }

  function openTaskMenu(event: CustomEvent<{ id: string; x: number; y: number }>, nodeId: string): void {
    diaryMenu = null;
    taskMenu = { id: event.detail.id, nodeId, x: event.detail.x, y: event.detail.y };
  }

  /** 卡片上的「日期与提醒」浮层：写操作与列表页共用 actions.setTaskSchedule */
  function setTaskSchedule(
    event: CustomEvent<{ id: string; dueDate: string; dueTime: string; reminders: ReminderRule[] }>
  ): void {
    const { id, ...patch } = event.detail;
    void setTaskScheduleAction(id, patch);
  }

  function withTaskTags(id: string, next: Tag[]): void {
    void replaceTaskTagsAction(id, next);
  }

  function removeTaskTag(event: CustomEvent<{ id: string; tagId: string }>): void {
    const task = findTask(event.detail.id);
    if (task) withTaskTags(task.id, task.tags.filter((tag) => tag.id !== event.detail.tagId));
  }

  function editTaskTag(event: CustomEvent<{ id: string; tagId: string; text: string }>): void {
    const task = findTask(event.detail.id);
    if (!task) return;
    withTaskTags(
      task.id,
      task.tags.map((tag) => (tag.id === event.detail.tagId ? { ...tag, text: event.detail.text || undefined } : tag))
    );
  }

  function removeTaskEmoji(event: CustomEvent<{ id: string; index: number }>): void {
    const task = findTask(event.detail.id);
    if (task) void replaceTaskEmojisAction(task.id, task.emojis.filter((_, i) => i !== event.detail.index));
  }

  function pickTaskEmoji(event: CustomEvent<{ id: string; index: number }>): void {
    // 表情选择器是 App 层的全局浮层，这里只负责把目标写进 store
    closeMenus();
    taskEmojiPicker.set({ taskId: event.detail.id, index: event.detail.index });
  }

  // ---- 日记卡片 ----
  function handleDiaryExpand(event: CustomEvent<{ id: string; expanded: boolean }>): void {
    void setDiaryUiAction(event.detail.id, { expanded: event.detail.expanded });
  }

  function openDiaryEntry(id: string): void {
    closeMenus();
    diaryEditor.set({ id });
  }

  // ---- 记账卡片 ----
  /** 记账结果卡的点击：打开记账面板改这一笔（与日记编辑器同一条浮层机制）。 */
  function openLedgerEntry(id: string): void {
    closeMenus();
    ledgerEditor.set({ id });
  }

  function openDiaryMenu(event: CustomEvent<{ id: string; x: number; y: number }>): void {
    taskMenu = null;
    diaryMenu = { id: event.detail.id, x: event.detail.x, y: event.detail.y };
  }

  // ---- 结果菜单里的动作 ----
  /** 跳到这条结果所在的界面（搜索词一并清掉，否则跳过去还是过滤后的列表）。 */
  function goToTask(nodeId: string): void {
    closeMenus();
    searchQuery.set("");
    void selectNodeAction(nodeId);
    showMobileContent();
  }

  function goToDiary(): void {
    closeMenus();
    searchQuery.set("");
    showMobileDiary();
  }

  function removeTask(id: string): void {
    closeMenus();
    void deleteTaskAction(id);
  }

  function removeDiary(id: string): void {
    closeMenus();
    void deleteDiaryEntry(id);
  }

  /** 原生预览独立于工作区：搜索结果也遵守应用内/系统浏览器偏好。 */
  function openLink(href: string): void {
    openLinkWithPreference(href, $appSettings.appearance.linkOpenMode)
      .catch((error) => showToast(`打开链接失败：${String(error)}`));
  }

  /** 供 Sidebar 的「点空白处收起浮层」调用。 */
  export function closeOverlays(): void {
    closeMenus();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<section class="search-results" bind:this={resultsEl}>
  {#if $searchHits.length === 0}
    <div class="search-results-empty">
      {#if $searchScanning}
        <span>搜索中…</span>
      {:else}
        <NotebookPen size={22} />
        <span>没有匹配的内容</span>
      {/if}
    </div>
  {:else}
    <!-- 结果同样走 VirtualStack（v0.8.6 需求 1）：命中封顶 200 条，只有视口附近在树上 -->
    <VirtualStack
      items={$searchHits}
      keyOf={(hit) => (hit as SearchHit).key}
      scroller={resultsEl}
      estimate={84}
      overscan={8}
    >
      <svelte:fragment slot="item" let:row>
        {@const hit = row as SearchHit}
        {#if hit.kind === "task"}
          <div class="search-hit" style={`--accent: ${hitAccent(hit.task.nodeId)}`}>
            <TaskCard
              task={hit.task}
              nodeId={hit.task.nodeId}
              cardStyle={hit.cardStyle}
              selected={taskMenu?.id === hit.task.id}
              on:toggle={(event) => toggleTask(event.detail)}
              on:expand={handleTaskExpand}
              on:edit={(event) => openTaskEditor(event.detail)}
              on:context={(event) => openTaskMenu(event, hit.task.nodeId)}
              on:openLink={(event) => openLink(event.detail.href)}
              on:setSchedule={setTaskSchedule}
              on:removeTag={removeTaskTag}
              on:editTag={editTaskTag}
              on:removeEmoji={removeTaskEmoji}
              on:pickEmoji={pickTaskEmoji}
            />
          </div>
        {:else if hit.kind === "diary"}
          <div class="search-hit" style={`--accent: ${diaryHitAccent}`}>
            <DiaryCard
              entry={hit.entry}
              today={todayIso()}
              selected={diaryMenu?.id === hit.entry.id}
              on:expand={handleDiaryExpand}
              on:edit={(event) => openDiaryEntry(event.detail)}
              on:context={openDiaryMenu}
              on:openLink={(event) => openLink(event.detail.href)}
            />
          </div>
        {:else}
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
  {/if}
</section>

{#if taskMenu}
  <ContextMenu x={taskMenu.x} y={taskMenu.y} minWidth={208} onClose={closeMenus}>
    <MenuItem icon={PenLine} label="编辑" onSelect={() => openTaskEditor(taskMenu?.id ?? "")} />
    <MenuItem icon={ExternalLink} label="打开所在列表" onSelect={() => goToTask(taskMenu?.nodeId ?? "")} />
    {#if taskMenuTask}
      <MenuItem
        icon={taskMenuTask.pinned ? PinOff : Pin}
        label={taskMenuTask.pinned ? "取消置顶" : "置顶"}
        onSelect={() => { if (taskMenuTask) void updateTaskAction(taskMenuTask.id, { pinned: !taskMenuTask.pinned }); closeMenus(); }}
      />
    {/if}
    <MenuSeparator />
    <MenuItem icon={Trash2} danger label="删除" onSelect={() => removeTask(taskMenu?.id ?? "")} />
  </ContextMenu>
{/if}

{#if diaryMenu && diaryEntry}
  <DiaryEntryMenu
    x={diaryMenu.x}
    y={diaryMenu.y}
    entry={diaryEntry}
    today={todayIso()}
    on:edit={(event) => openDiaryEntry(event.detail)}
    on:close={closeMenus}
  />
{/if}
