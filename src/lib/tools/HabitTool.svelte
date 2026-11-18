<script lang="ts">
  /**
   * 习惯打卡（v0.9.3）：工具箱子页。两个视图——
   * - 全部：进行中 / 已暂停 / 已完成 三个计数页签 + 习惯卡片（图标圆 + 名称 + 频率 + 累计打卡）；
   * - 每日：日期导航 + 全天/上午/下午/晚上 分组的图标圆阵，点圆 = 打卡/取消（乐观更新）。
   * 点卡片进统计面板（HabitStats），右键/长按开动作菜单；新建走子页头部加号（habitNew 递话）。
   * 样式全部 `.habit-*` 前缀住 toolbox.css，移动端覆盖住 mobile.css。
   */
  import { CalendarCheck, ChevronLeft, ChevronRight, Pencil, Pause, Play, CheckCircle2, Trash2 } from "@lucide/svelte";
  import { get } from "svelte/store";
  import { habits, habitNew, showToast, todayIso } from "../stores";
  import { deleteHabit, setHabitCheckin, updateHabit } from "../actions";
  import { dailyGroups, freqLabel, isChecked, timeLabel } from "../habits";
  import { ledgerIcon, softColor } from "../ledgerIcons";
  import { longpress } from "../longpress";
  import ContextMenu from "../menu/ContextMenu.svelte";
  import MenuItem from "../menu/MenuItem.svelte";
  import MenuSeparator from "../menu/MenuSeparator.svelte";
  import HabitEditor from "./habit/HabitEditor.svelte";
  import HabitStats from "./habit/HabitStats.svelte";
  import type { Habit, HabitStatus } from "../types";

  let view: "all" | "daily" = "all";
  let statusTab: HabitStatus = "active";
  let day = todayIso();
  let editorTarget: { id?: string } | null = null;
  let statsId: string | null = null;
  let menu: { x: number; y: number; anchor: HTMLElement | null; habit: Habit } | null = null;
  let confirmDelete: string | null = null;
  // 初值取当前计数（与草稿纸同写法）：否则重进工具页会把上一次的加号请求再执行一遍
  let seenNew = get(habitNew);

  $: list = $habits;
  $: today = todayIso();
  $: counts = {
    active: list.filter((habit) => habit.status === "active").length,
    paused: list.filter((habit) => habit.status === "paused").length,
    done: list.filter((habit) => habit.status === "done").length
  };
  $: shown = list.filter((habit) => habit.status === statusTab);
  $: groups = dailyGroups(list, day);
  $: statsHabit = statsId ? list.find((habit) => habit.id === statsId) ?? null : null;
  // 子页头部加号经 store 递话（与草稿纸同一套路）；计数增加 = 请求新建
  $: if ($habitNew > seenNew) {
    seenNew = $habitNew;
    editorTarget = {};
  }
  // 统计面板开着的习惯被删掉（同步/菜单）→ 收面板。判据直接读 list，
  // 不读 statsHabit——否则 statsHabit → statsId → statsHabit 成响应式环
  $: if (statsId && !list.some((habit) => habit.id === statsId)) statsId = null;

  const TABS: Array<{ id: HabitStatus; label: string }> = [
    { id: "active", label: "进行中" },
    { id: "paused", label: "已暂停" },
    { id: "done", label: "已完成" }
  ];

  function habitVars(habit: Habit): string {
    const main = habit.color || "var(--accent)";
    const soft = habit.color ? softColor(habit.color, 0.16) : "rgb(0 0 0 / 5%)";
    return `--habit-main: ${main}; --habit-soft: ${soft}`;
  }

  function openMenu(event: MouseEvent | { x: number; y: number }, habit: Habit): void {
    const mouse = event as MouseEvent;
    menu = {
      x: mouse.clientX ?? (event as { x: number }).x,
      y: mouse.clientY ?? (event as { y: number }).y,
      // 长按只有触点坐标：anchor 留 null 走点锚定（ContextMenu 支持）
      anchor: (mouse.currentTarget as HTMLElement | null) ?? null,
      habit
    };
    confirmDelete = null;
  }

  function openCardMenu(event: MouseEvent, habit: Habit): void {
    event.preventDefault();
    openMenu(event, habit);
  }

  async function toggleCheck(habit: Habit, date: string): Promise<void> {
    const ok = await setHabitCheckin(habit.id, date, !isChecked(habit, date));
    if (ok && !isChecked(habit, date)) showToast(`「${habit.name}」已打卡`, 1600);
  }

  async function setStatus(habit: Habit, status: HabitStatus): Promise<void> {
    menu = null;
    const ok = await updateHabit(habit.id, { status });
    if (ok) {
      showToast(
        status === "paused" ? `「${habit.name}」已暂停` : status === "done" ? `「${habit.name}」已完成` : `「${habit.name}」已恢复`,
        1800
      );
    }
  }

  async function removeHabit(habit: Habit): Promise<void> {
    menu = null;
    confirmDelete = null;
    const ok = await deleteHabit(habit.id);
    if (ok) showToast(`习惯「${habit.name}」已删除`, 1800);
  }

  function stepDay(delta: number): void {
    const [y, m, d] = day.split("-").map(Number);
    const base = new Date(y, m - 1, d);
    base.setDate(base.getDate() + delta);
    const pad = (n: number): string => String(n).padStart(2, "0");
    day = `${base.getFullYear()}-${pad(base.getMonth() + 1)}-${pad(base.getDate())}`;
  }

  function dayTitle(date: string): string {
    return `${Number(date.slice(5, 7))}/${Number(date.slice(8, 10))}`;
  }
</script>

<div class="habit-tool">
  <div class="habit-view-switch segmented" role="tablist" aria-label="习惯视图">
    <button type="button" role="tab" class:active={view === "all"} aria-selected={view === "all"} on:click={() => (view = "all")}>全部</button>
    <button type="button" role="tab" class:active={view === "daily"} aria-selected={view === "daily"} on:click={() => (view = "daily")}>每日</button>
  </div>

  {#if view === "all"}
    <div class="habit-tabs" role="tablist" aria-label="习惯状态">
      {#each TABS as tab (tab.id)}
        <button
          type="button"
          role="tab"
          class="habit-tab"
          class:active={statusTab === tab.id}
          aria-selected={statusTab === tab.id}
          on:click={() => (statusTab = tab.id)}
        >
          <span class="habit-tab-label">{tab.label}</span>
          <strong class="habit-tab-count">{counts[tab.id]}</strong>
        </button>
      {/each}
    </div>

    <div class="habit-list">
      {#if shown.length === 0}
        <p class="habit-empty">
          {statusTab === "active" ? "还没有进行中的习惯，点右上角 + 新建一个" : statusTab === "paused" ? "没有已暂停的习惯" : "没有已完成的习惯"}
        </p>
      {/if}
      {#each shown as habit (habit.id)}
        <button
          type="button"
          class="habit-card"
          class:muted={habit.status !== "active"}
          style={habitVars(habit)}
          title="查看统计"
          on:click={() => (statsId = habit.id)}
          on:contextmenu={(event) => openCardMenu(event, habit)}
          use:longpress={(pos) => openMenu(pos, habit)}
        >
          <span class="habit-card-icon">
            <svelte:component this={ledgerIcon(habit.icon, "CalendarCheck")} size={22} />
          </span>
          <span class="habit-card-mid">
            <span class="habit-card-name">{habit.name}</span>
            <span class="habit-card-sub">{freqLabel(habit.freq)} · {timeLabel(habit.time)}</span>
          </span>
          <span class="habit-card-stat">
            <strong>{habit.checkins.length}天</strong>
            <span>累计打卡</span>
          </span>
        </button>
      {/each}
    </div>
  {:else}
    <div class="habit-day-bar">
      <button type="button" class="toolbox-icon-button" title="前一天" aria-label="前一天" on:click={() => stepDay(-1)}>
        <ChevronLeft size={18} />
      </button>
      <strong class="habit-day-title">{dayTitle(day)}</strong>
      <button type="button" class="toolbox-icon-button" title="后一天" aria-label="后一天" on:click={() => stepDay(1)}>
        <ChevronRight size={18} />
      </button>
      {#if day !== today}
        <button type="button" class="habit-today-chip" on:click={() => (day = today)}>今天</button>
      {/if}
    </div>

    <div class="habit-daily">
      {#if groups.length === 0}
        <p class="habit-empty">这一天没有计划内的习惯</p>
      {/if}
      {#each groups as group (group.slot)}
        <section class="habit-slot">
          <header class="habit-slot-head"><span>{group.label}</span></header>
          <div class="habit-slot-grid">
            {#each group.habits as habit (habit.id)}
              <button
                type="button"
                class="habit-dot"
                class:checked={isChecked(habit, day)}
                style={habitVars(habit)}
                title={isChecked(habit, day) ? `取消 ${dayTitle(day)} 的打卡` : `打卡 ${dayTitle(day)}`}
                aria-pressed={isChecked(habit, day)}
                on:click={() => void toggleCheck(habit, day)}
                on:contextmenu={(event) => openCardMenu(event, habit)}
                use:longpress={(pos) => openMenu(pos, habit)}
              >
                <span class="habit-dot-circle">
                  <svelte:component this={ledgerIcon(habit.icon, "CalendarCheck")} size={26} />
                </span>
                <span class="habit-dot-name">{habit.name}</span>
              </button>
            {/each}
          </div>
        </section>
      {/each}
    </div>
  {/if}
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} xAlign="left" anchor={menu.anchor} onClose={() => (menu = null)}>
    {#if confirmDelete === menu.habit.id}
      <MenuItem icon={Trash2} label={`确认删除「${menu.habit.name}」？`} danger onSelect={() => { const habit = menu?.habit; if (habit) void removeHabit(habit); }} />
      <MenuItem label="取消" onSelect={() => (confirmDelete = null)} />
    {:else}
      {#if menu.habit.status === "active"}
        <MenuItem
          icon={CheckCircle2}
          label={isChecked(menu.habit, today) ? "取消今天的打卡" : "打卡今天"}
          onSelect={() => {
            const habit = menu?.habit;
            menu = null;
            if (habit) void toggleCheck(habit, today);
          }}
        />
        <MenuSeparator />
      {/if}
      <MenuItem icon={Pencil} label="编辑" onSelect={() => { const habit = menu?.habit; menu = null; if (habit) editorTarget = { id: habit.id }; }} />
      <MenuItem icon={CalendarCheck} label="统计" onSelect={() => { const habit = menu?.habit; menu = null; if (habit) statsId = habit.id; }} />
      {#if menu.habit.status === "active"}
        <MenuItem icon={Pause} label="暂停" onSelect={() => { const habit = menu?.habit; if (habit) void setStatus(habit, "paused"); }} />
        <MenuItem icon={CheckCircle2} label="标记完成" onSelect={() => { const habit = menu?.habit; if (habit) void setStatus(habit, "done"); }} />
      {:else if menu.habit.status === "paused"}
        <MenuItem icon={Play} label="恢复进行中" onSelect={() => { const habit = menu?.habit; if (habit) void setStatus(habit, "active"); }} />
        <MenuItem icon={CheckCircle2} label="标记完成" onSelect={() => { const habit = menu?.habit; if (habit) void setStatus(habit, "done"); }} />
      {:else}
        <MenuItem icon={Play} label="恢复进行中" onSelect={() => { const habit = menu?.habit; if (habit) void setStatus(habit, "active"); }} />
      {/if}
      <MenuSeparator />
      <MenuItem icon={Trash2} label="删除" danger onSelect={() => { const habit = menu?.habit; if (habit) confirmDelete = habit.id; }} />
    {/if}
  </ContextMenu>
{/if}

{#if statsHabit}
  <HabitStats
    habit={statsHabit}
    onClose={() => (statsId = null)}
    onEdit={() => { const id = statsHabit?.id; statsId = null; if (id) editorTarget = { id }; }}
  />
{/if}

{#if editorTarget}
  <HabitEditor target={editorTarget} onClose={() => (editorTarget = null)} />
{/if}