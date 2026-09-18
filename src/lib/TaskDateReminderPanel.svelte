<script lang="ts">
  /**
   * 「日期与提醒」面板（v0.8.3）：一个任务的**日期 / 时刻 / 提醒**三样都在这里配。
   *
   * 三个入口共用同一份组件——右键菜单的「日期与提醒」、卡片上点日期或时刻、
   * 编辑器工具栏的日期按钮（需求 13 说的就是「都是需要复用的逻辑」）。三处各写一份
   * 的话，「点了日期就关掉」这类交互差异会立刻分叉。
   *
   * **它不是点一下就关的浮层**：里面有日历、时刻行、提醒行三组配置，只有底部的
   * 「清除 / 保存」与点击面板外部才会关闭。时刻的双轨滚轮与「自定义提醒」都是
   * 面板**内部换页**（不是再套一层菜单）——嵌套三层子菜单在桌面上会顶出屏幕右缘，
   * 在移动端则要钻三层才退得回来。
   *
   * 所有编辑都发生在本地草稿上，点「保存」才写命令层：面板里连着改日期、时刻、
   * 三条提醒，逐样落盘就是三次写 + 三次快照往返（v0.8.2 刚为「一次写入让 store
   * 换两次身份」付过代价）。
   */
  import { onDestroy } from "svelte";
  import { Bell, CalendarDays, ChevronLeft, Clock, Plus, X } from "@lucide/svelte";
  import CalendarGrid from "./CalendarGrid.svelte";
  import TimePicker from "./TimePicker.svelte";
  import { formatClock, nowClock, parseClock } from "./clock";
  import { addBackInterceptor } from "./platform";
  import {
    REMINDER_PRESETS,
    absoluteReminderFromLocal,
    canUseBeforeDue,
    presetReminder,
    reminderLabel,
    reminderMoment,
    sortReminders,
    withReminder
  } from "./reminders";
  import type { ReminderPresetId } from "./reminders";
  import type { ReminderRule } from "./types";

  /** 截止日期 YYYY-MM-DD；空 = 没有日期 */
  export let dueDate = "";
  /** 截止时刻 HH:MM；空 = 只精确到天 */
  export let dueTime = "";
  export let reminders: ReminderRule[] = [];
  /**
   * 嵌在别人的浮层里（右键菜单的子面板）时为 true：那时「点外部关闭」与最外一层
   * 返回键都归宿主（`ContextMenu`）管，本面板的返回键只逐级退自己的内部视图。
   */
  export let embedded = false;
  /** 保存：交出三样配置的最终值（宿主负责写命令层并关闭面板） */
  export let onSave: (patch: { dueDate: string; dueTime: string; reminders: ReminderRule[] }) => void = () => {};
  /** 清除：日期、时刻、提醒全部恢复默认，然后退出 */
  export let onClear: () => void = () => {};
  export let onClose: () => void = () => {};

  // ---- 草稿（点「保存」才写出去；点外部关掉 = 放弃这次编辑） ----
  let date = dueDate;
  let clockValue = dueTime;
  let rules: ReminderRule[] = [...reminders];

  let view: "main" | "time" | "custom" = "main";
  let addMenuOpen = false;
  let customTab: "date" | "time" = "date";
  let customDate = "";
  let customError = "";

  let grid: CalendarGrid;
  let customGrid: CalendarGrid;
  /** 双轨滚轮的草稿：主面板与自定义面板不会同时开着，共用一份就够 */
  let wheel = { hour: 0, minute: 0 };
  /** 触屏上没有 hover：点一下先把删除叉露出来（与任务卡片的标签同一套手势） */
  let revealedChip = -1;

  function todayIso(): string {
    const now = new Date();
    return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  }

  $: parsedClock = parseClock(clockValue);
  /** 「截止前 N 分钟」要有日期与分钟时刻才算得出瞬时 */
  $: beforeDueReady = canUseBeforeDue(date, clockValue);
  $: chips = sortReminders(rules, date, clockValue);

  // ---- 时刻 ----

  /** 进双轨视图：设过时刻就停在那个值，没设过就停在**此刻**（需求 10.3.1）。 */
  function openTimeView(): void {
    const parsed = parseClock(clockValue);
    const now = new Date();
    wheel = { hour: parsed?.hour ?? now.getHours(), minute: parsed?.minute ?? now.getMinutes() };
    view = "time";
  }

  function confirmTime(): void {
    clockValue = formatClock(wheel.hour, wheel.minute);
    // 时刻必须有日期依附（core 的 DUE_DATE_REQUIRED）：没有就落在今天，与「添加日期」
    // 的老语义一致，而且**在日历上看得见**（不是等到保存时才偷偷补一个日期）。
    if (!date) date = todayIso();
    view = "main";
  }

  function clearTime(): void {
    clockValue = "";
    // 「截止前」提醒失去依附对象，留着就是一条永远算不出瞬时的死规则（core 同口径清掉）
    rules = rules.filter((rule) => rule.kind !== "beforeDue");
    view = "main";
  }

  // ---- 提醒 ----

  function pickPreset(id: ReminderPresetId): void {
    addMenuOpen = false;
    if (id === "custom") {
      openCustom();
      return;
    }
    const rule = presetReminder(id);
    if (rule) rules = withReminder(rules, rule, date, clockValue);
  }

  function openCustom(): void {
    customDate = date || todayIso();
    const parsed = parseClock(clockValue);
    const now = new Date();
    wheel = {
      hour: parsed?.hour ?? now.getHours(),
      minute: parsed?.minute ?? now.getMinutes()
    };
    customTab = "date";
    customError = "";
    view = "custom";
  }

  function confirmCustom(): void {
    const clockText = formatClock(wheel.hour, wheel.minute);
    const rule = absoluteReminderFromLocal(customDate, clockText);
    if (!rule) {
      customError = "日期或时刻不合法";
      return;
    }
    const at = reminderMoment(rule, date, clockValue);
    // core 会以 REMINDER_IN_PAST 拒掉新增的过时提醒（已错过的不补发）。
    // 在界面上先说清楚：省一次往返，也省一次「保存失败」的困惑。
    if (at !== null && at <= Date.now()) {
      customError = "这个时刻已经过去，提醒不会触发";
      return;
    }
    rules = withReminder(rules, rule, date, clockValue);
    revealedChip = -1;
    view = "main";
  }

  function removeReminder(index: number): void {
    const target = chips[index];
    if (!target) return;
    const at = reminderMoment(target, date, clockValue);
    // chips 是排过序的副本，按「同一条规则 / 同一瞬时」回到原数组里定位
    const found = rules.findIndex(
      (rule) => rule === target || reminderMoment(rule, date, clockValue) === at
    );
    if (found < 0) return;
    rules = rules.filter((_, position) => position !== found);
    revealedChip = -1;
  }

  // ---- 返回键 ----
  // 这里用 addBackInterceptor 而不是 createBackGuard：**要不要消费**取决于当前在哪一层
  // （内部视图退一级 = 消费；已经在主视图且嵌在菜单里 = 不消费，交给 ContextMenu 去收
  // 整个菜单）。createBackGuard 的回调一律消费，表达不了这个分叉。注销配对写在
  // onDestroy 里——漏掉的后果是返回键被一个看不见的面板永久吃掉。
  const releaseBack = addBackInterceptor(() => {
    if (addMenuOpen) {
      addMenuOpen = false;
      return true;
    }
    if (view === "custom") {
      if (customTab === "date" && customGrid?.back()) return true;
      view = "main";
      return true;
    }
    if (view === "time") {
      view = "main";
      return true;
    }
    if (grid?.back()) return true;
    if (embedded) return false;
    onClose();
    return true;
  });
  onDestroy(releaseBack);
</script>

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<!-- stopPropagation：宿主（卡片浮层 / 菜单）都靠「点到外面就关」，面板内部的点击
     绝不能冒到 window 去，否则点日历的第一下就把整个面板关掉了 -->
<div class="date-reminder-panel" on:click|stopPropagation={() => (addMenuOpen = false)} on:mousedown|stopPropagation>
  {#if view === "time"}
    <div class="date-picker-header">
      <button type="button" on:click={() => (view = "main")} aria-label="返回"><ChevronLeft size={16} /></button>
      <span class="dp-title">选择时刻</span>
      <span class="dp-head-spacer" aria-hidden="true"></span>
    </div>
    <div class="dp-time-wheel">
      <TimePicker hour={wheel.hour} minute={wheel.minute} on:change={(event) => (wheel = event.detail)} />
    </div>
    <div class="date-picker-actions">
      <button type="button" class="dp-clear" on:click={clearTime}>清除</button>
      <button type="button" class="dp-today" on:click={confirmTime}>确认</button>
    </div>
  {:else if view === "custom"}
    <div class="date-picker-header">
      <button type="button" on:click={() => (view = "main")} aria-label="返回"><ChevronLeft size={16} /></button>
      <span class="dp-title">自定义提醒</span>
      <span class="dp-head-spacer" aria-hidden="true"></span>
    </div>
    <div class="dr-tabs" role="tablist" aria-label="自定义提醒">
      <button type="button" role="tab" class="dr-tab" aria-selected={customTab === "date"} class:active={customTab === "date"} on:click={() => (customTab = "date")}>日期</button>
      <button type="button" role="tab" class="dr-tab" aria-selected={customTab === "time"} class:active={customTab === "time"} on:click={() => (customTab = "time")}>时间</button>
    </div>
    {#if customTab === "date"}
      <CalendarGrid bind:this={customGrid} value={customDate} initial={customDate} on:select={(event) => (customDate = event.detail)} />
    {:else}
      <div class="dp-time-wheel">
        <TimePicker hour={wheel.hour} minute={wheel.minute} on:change={(event) => (wheel = event.detail)} />
      </div>
    {/if}
    {#if customError}<div class="dr-error" role="alert">{customError}</div>{/if}
    <div class="date-picker-actions">
      <button type="button" class="dp-clear" on:click={() => (view = "main")}>取消</button>
      <button type="button" class="dp-today" on:click={confirmCustom}>确定</button>
    </div>
  {:else}
    <CalendarGrid bind:this={grid} value={date} initial={date} on:select={(event) => (date = event.detail)} />

    <div class="dr-row">
      <button type="button" class="dp-time-trigger" title={parsedClock ? "修改时间" : "添加时间"} on:click={openTimeView}>
        <Clock size={15} />
        {#if parsedClock}
          <strong>{formatClock(parsedClock.hour, parsedClock.minute)}</strong>
        {:else}
          <span class="dr-placeholder">添加时间</span>
        {/if}
      </button>
      {#if parsedClock}
        <button type="button" class="dr-row-clear" aria-label="清除时间" title="清除时间" on:click={clearTime}>
          <X size={13} strokeWidth={2.5} />
        </button>
      {/if}
    </div>

    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="dr-row dr-reminders" on:click|stopPropagation>
      {#if chips.length === 0}
        <button type="button" class="dp-time-trigger dr-reminder-empty" title="添加提醒" on:click|stopPropagation={() => (addMenuOpen = !addMenuOpen)}>
          <Bell size={15} />
          <span class="dr-placeholder">添加提醒</span>
        </button>
      {:else}
        <span class="dr-row-icon" aria-hidden="true"><Bell size={15} /></span>
        <div class="dr-reminder-chips">
          {#each chips as rule, index (index)}
            <span
              class="dr-reminder-chip"
              class:reveal-delete={revealedChip === index}
              title={reminderLabel(rule, date, clockValue)}
              on:click|stopPropagation={() => (revealedChip = revealedChip === index ? -1 : index)}
            >
              {reminderLabel(rule, date, clockValue)}
              <button
                type="button"
                class="tag-delete"
                aria-label="移除提醒"
                on:click|stopPropagation={() => removeReminder(index)}
              ><X size={9} strokeWidth={3} /></button>
            </span>
          {/each}
        </div>
        <button type="button" class="dr-reminder-plus" aria-label="添加提醒" title="添加提醒" on:click|stopPropagation={() => (addMenuOpen = !addMenuOpen)}>
          <Plus size={14} />
        </button>
      {/if}

      {#if addMenuOpen}
        <div class="dr-add-menu" role="menu">
          {#each REMINDER_PRESETS as preset (preset.id)}
            {@const blocked = preset.needsDueTime && !beforeDueReady}
            <button
              type="button"
              class="menu-item-button"
              data-menu-item
              disabled={blocked}
              title={blocked ? "需要先设置精确到分钟的截止时间" : undefined}
              on:click|stopPropagation={() => pickPreset(preset.id)}
            >
              {#if preset.id === "custom"}<CalendarDays size={14} />{:else}<Bell size={14} />{/if}
              <span class="menu-item-label">{preset.label}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>

    <div class="date-picker-actions">
      <button type="button" class="dp-clear" on:click={() => onClear()}>清除</button>
      <button type="button" class="dp-today" on:click={() => onSave({ dueDate: date, dueTime: clockValue, reminders: rules })}>保存</button>
    </div>
  {/if}
</div>
