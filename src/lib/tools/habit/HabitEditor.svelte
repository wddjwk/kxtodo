<script lang="ts">
  /**
   * 习惯编辑器（v0.9.3）：设置式表单，骨架与日程分类面板同一套
   * （editor-overlay + editor-dialog + calendar-sheet，返回键/Esc/点遮罩/安全区都现成）。
   * 行：名称 / 图标 / 颜色 / 计划 / 时间 / 提醒。行内浮层一律 ContextMenu 宿主
   * （壳内定位、四边钳制、移动端钻入），图标选择器懒加载 IconPicker（icon 模式）。
   */
  import { Bell, CalendarDays, Check, ChevronRight, Palette, Plus, Repeat, Trash2, X } from "@lucide/svelte";
  import { showToast, todayIso } from "../../stores";
  import { addHabit, deleteHabit, findHabit, updateHabit } from "../../actions";
  import { freqLabel, HABIT_WEEKDAY_LABELS, timeLabel } from "../../habits";
  import { reminderLabel } from "../../calendar";
  import { ledgerIcon, softColor } from "../../ledgerIcons";
  import { openColorPicker } from "../../colorPickerPanel";
  import { createBackGuard } from "../../platform";
  import { imeInset } from "../../imeInset";
  import { suppressGhostClick } from "../../ghostClick";
  import { nowClock } from "../../clock";
  import ContextMenu from "../../menu/ContextMenu.svelte";
  import MenuItem from "../../menu/MenuItem.svelte";
  import MenuSeparator from "../../menu/MenuSeparator.svelte";
  import NumberField from "../../NumberField.svelte";
  import type { HabitFreq, ReminderRule } from "../../types";

  export let target: { id?: string } = {};
  export let onClose: () => void = () => {};

  const PALETTE = [
    "#7c5cbf", "#3d8bfd", "#2f9e6e", "#e0654f",
    "#d94f70", "#e8a33d", "#9b59b6", "#7cb342",
    "#0f9b9b", "#8d6e63", "#7f8c8d", "#34495e"
  ];
  const REMINDER_PRESETS = [0, 5, 15, 30, 60];

  const editing = target.id ? findHabit(target.id) : undefined;
  let name = editing?.name ?? "";
  let color = editing?.color ?? PALETTE[0];
  let icon = editing?.icon ?? "CalendarDays";
  let freq: HabitFreq = editing?.freq ?? { kind: "daily", weekdays: [] };
  let targetCount = editing?.targetCount ?? 1;
  let time = editing?.time ?? "";
  let reminders: ReminderRule[] = editing?.reminders ?? [];
  let busy = false;
  let confirmDelete = false;
  let iconPickerOpen = false;
  let picker: null | "freq" | "time" | "reminder" = null;
  let pickerAt = { x: 0, y: 0 };
  let pickerAnchor: HTMLElement | null = null;
  let reminderDate = todayIso();
  let reminderTime = nowClock();

  $: nameTrimmed = name.trim();
  $: freqText = freqLabel(freq);
  $: previewSoft = color ? softColor(color, 0.16) : "rgb(0 0 0 / 5%)";

  const backGuard = createBackGuard();
  $: backGuard(true, onClose);

  function togglePicker(which: "freq" | "time" | "reminder", anchor: HTMLElement): void {
    if (picker === which) {
      picker = null;
      return;
    }
    const rect = anchor.getBoundingClientRect();
    pickerAt = { x: rect.left, y: rect.bottom + 6 };
    pickerAnchor = anchor;
    picker = which;
  }

  function closePicker(): void {
    picker = null;
  }

  function toggleWeekday(day: number): void {
    const has = freq.weekdays.includes(day);
    const weekdays = has ? freq.weekdays.filter((item) => item !== day) : [...freq.weekdays, day].sort((a, b) => a - b);
    freq = weekdays.length === 0 ? { kind: "daily", weekdays: [] } : { kind: "weekly", weekdays };
  }

  function pickCustomColor(anchor: HTMLElement): void {
    openColorPicker({
      key: "habit:color",
      color: color || PALETTE[0],
      anchor,
      onPreview: (next) => (color = next),
      onConfirm: (next) => (color = next),
      onCancel: () => undefined
    });
  }

  function addPresetReminder(minutes: number): void {
    if (reminders.some((rule) => rule.kind === "beforeDue" && rule.minutes === minutes)) {
      showToast("这条提醒已经加过了");
      return;
    }
    reminders = [...reminders, { kind: "beforeDue", minutes }];
    closePicker();
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

  function reminderText(rule: ReminderRule): string {
    if (rule.kind === "beforeDue" && rule.minutes === 0) return "计划时刻";
    return reminderLabel(rule);
  }

  async function save(): Promise<void> {
    if (busy || nameTrimmed === "") return;
    busy = true;
    try {
      const draft = {
        name: nameTrimmed,
        color,
        icon,
        freq,
        time,
        reminders,
        targetCount
      };
      if (editing) {
        const ok = await updateHabit(editing.id, draft);
        if (ok) {
          showToast("习惯已保存");
          onClose();
        }
        return;
      }
      const created = await addHabit(draft);
      if (created) {
        showToast(`习惯「${created.name}」已添加`);
        onClose();
      }
    } finally {
      busy = false;
    }
  }

  async function remove(): Promise<void> {
    if (!editing || busy) return;
    busy = true;
    try {
      const ok = await deleteHabit(editing.id);
      if (ok) {
        showToast(`习惯「${editing.name}」已删除`);
        onClose();
      }
    } finally {
      busy = false;
    }
  }

  function handleBackdropPointerDown(event: PointerEvent): void {
    if (event.target !== event.currentTarget) return;
    event.preventDefault();
    suppressGhostClick({ x: event.clientX, y: event.clientY });
    onClose();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented) return;
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
<div class="editor-overlay" use:imeInset on:pointerdown={handleBackdropPointerDown}>
  <div
    class="editor-dialog calendar-sheet habit-sheet"
    role="dialog"
    aria-label={editing ? "编辑习惯" : "新建习惯"}
    tabindex="-1"
    on:pointerdown|stopPropagation
    on:click|stopPropagation
  >
    <header class="calendar-sheet-head">
      <span class="calendar-sheet-title">{editing ? "编辑习惯" : "新建习惯"}</span>
      <span class="calendar-sheet-spacer"></span>
      <button type="button" class="editor-icon-button" title="关闭" aria-label="关闭" on:click={onClose}>
        <X size={17} />
      </button>
    </header>

    <div class="calendar-sheet-body">
      <div class="cal-field">
        <span class="cal-field-label">名称</span>
        <input
          class="habit-input"
          type="text"
          maxlength="40"
          placeholder="习惯名称"
          bind:value={name}
          on:keydown={(event) => {
            if (event.key === "Enter" && !event.isComposing && event.keyCode !== 229) {
              event.preventDefault();
              void save();
            }
          }}
        />
      </div>

      <div class="cal-field">
        <span class="cal-field-label">图标</span>
        <button type="button" class="cal-field-trigger" on:click={() => (iconPickerOpen = true)}>
          <span class="habit-icon-preview" style={`--habit-main: ${color || "var(--accent)"}; --habit-soft: ${previewSoft}`}>
            <svelte:component this={ledgerIcon(icon, "CalendarDays")} size={16} />
          </span>
          <span class="cal-field-value">{icon || "默认图标"}</span>
          <ChevronRight size={15} class="cal-field-chevron" />
        </button>
      </div>

      <div class="cal-field">
        <span class="cal-field-label">颜色</span>
        <div class="calendar-cat-colors">
          {#each PALETTE as preset (preset)}
            <button
              type="button"
              class="calendar-cat-dot"
              class:active={color.toLowerCase() === preset.toLowerCase()}
              style={`background: ${preset}`}
              title={preset}
              aria-label="使用颜色 {preset}"
              on:click={() => (color = preset)}
            >
              {#if color.toLowerCase() === preset.toLowerCase()}<Check size={13} />{/if}
            </button>
          {/each}
          <button
            type="button"
            class="calendar-cat-dot custom"
            title="自定义颜色"
            aria-label="自定义颜色"
            style={`--dot: ${color || PALETTE[0]}`}
            on:click={(event) => pickCustomColor(event.currentTarget)}
          ><Palette size={13} /></button>
        </div>
      </div>

      <div class="cal-field">
        <span class="cal-field-label">计划</span>
        <button
          type="button"
          class="cal-field-trigger"
          aria-expanded={picker === "freq"}
          on:click={(event) => togglePicker("freq", event.currentTarget)}
        >
          <Repeat size={15} />
          <span class="cal-field-value">{freqText}</span>
          <ChevronRight size={15} class="cal-field-chevron" />
        </button>
      </div>

      <div class="cal-field">
        <span class="cal-field-label">每日次数</span>
        <span class="habit-target-control">
          <NumberField value={targetCount} min={1} max={99} suffix="次" ariaLabel="每日目标打卡次数" onCommit={(value) => (targetCount = value)} />
          <span class="habit-target-hint">{targetCount > 1 ? `每天打满 ${targetCount} 次才算完成` : "每天打一次卡"}</span>
        </span>
      </div>

      <div class="cal-field">
        <span class="cal-field-label">时间</span>
        <button
          type="button"
          class="cal-field-trigger"
          aria-expanded={picker === "time"}
          on:click={(event) => togglePicker("time", event.currentTarget)}
        >
          <CalendarDays size={15} />
          <span class="cal-field-value">{timeLabel(time)}</span>
          <ChevronRight size={15} class="cal-field-chevron" />
        </button>
      </div>

      <div class="cal-field">
        <span class="cal-field-label">提醒</span>
        <span class="habit-reminder-control">
          {#each reminders as rule, index (index)}
            <span class="habit-reminder-chip">
              <Bell size={12} />
              {reminderText(rule)}
              <button type="button" class="habit-chip-x" title="移除提醒" aria-label="移除提醒" on:click={() => (reminders = reminders.filter((_, position) => position !== index))}>
                <X size={11} />
              </button>
            </span>
          {/each}
          <button
            type="button"
            class="cal-field-trigger habit-reminder-add"
            aria-expanded={picker === "reminder"}
            on:click={(event) => togglePicker("reminder", event.currentTarget)}
          >
            <Plus size={14} />
            <span class="cal-field-value">{reminders.length === 0 ? "添加提醒" : "继续添加"}</span>
          </button>
        </span>
      </div>
    </div>

    <footer class="calendar-sheet-foot">
      {#if editing}
        {#if confirmDelete}
          <button type="button" class="settings-button danger" disabled={busy} on:click={() => void remove()}>
            <Trash2 size={15} />确认删除
          </button>
          <button type="button" class="settings-button" on:click={() => (confirmDelete = false)}>取消</button>
        {:else}
          <button type="button" class="settings-button danger" disabled={busy} on:click={() => (confirmDelete = true)}>
            <Trash2 size={15} />删除
          </button>
        {/if}
      {/if}
      <span class="calendar-sheet-spacer"></span>
      <button type="button" class="settings-button" on:click={onClose}>取消</button>
      <button type="button" class="settings-button primary" disabled={busy || nameTrimmed === ""} on:click={() => void save()}>
        <Check size={15} />保存
      </button>
    </footer>
  </div>

  <!-- 行内浮层必须挂在 overlay 的叠层上下文里（ContextMenu z 3600 < overlay 4000，
       挂在 overlay 外会被遮罩整层盖住点不到——日程编辑器同一套写法） -->
  {#if picker === "freq"}
  <ContextMenu x={pickerAt.x} y={pickerAt.y} xAlign="left" mirrorXOnFlip={false} minWidth={180} anchor={pickerAnchor} onClose={closePicker}>
    <MenuItem checkable active={freq.kind === "daily"} label="每天" onSelect={() => (freq = { kind: "daily", weekdays: [] })} />
    <MenuSeparator />
    {#each HABIT_WEEKDAY_LABELS as label, day (day)}
      <MenuItem checkable active={freq.kind === "weekly" && freq.weekdays.includes(day)} label={`周${label}`} onSelect={() => toggleWeekday(day)} />
    {/each}
  </ContextMenu>
{:else if picker === "time"}
  <ContextMenu x={pickerAt.x} y={pickerAt.y} xAlign="left" mirrorXOnFlip={false} minWidth={180} anchor={pickerAnchor} onClose={closePicker}>
    <MenuItem checkable active={time === ""} label="全天" onSelect={() => { time = ""; closePicker(); }} />
    <MenuSeparator />
    {#each ["06:00", "07:00", "08:00", "09:00", "12:00", "14:00", "18:00", "19:00", "20:00", "21:00", "22:00"] as preset (preset)}
      <MenuItem checkable active={time === preset} label={preset} onSelect={() => { time = preset; closePicker(); }} />
    {/each}
    <MenuSeparator />
    <MenuItem icon={CalendarDays} label="自定义时刻…">
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
      <div slot="submenu" class="habit-custom-row" on:click|stopPropagation>
        <input type="time" bind:value={time} on:keydown|stopPropagation />
        <button type="button" class="menu-action-button" on:click={closePicker}><Check size={14} />确定</button>
      </div>
    </MenuItem>
  </ContextMenu>
{:else if picker === "reminder"}
  <ContextMenu x={pickerAt.x} y={pickerAt.y} xAlign="left" mirrorXOnFlip={false} minWidth={200} anchor={pickerAnchor} onClose={closePicker}>
    {#each REMINDER_PRESETS as minutes (minutes)}
      <MenuItem
        checkable
        active={reminders.some((rule) => rule.kind === "beforeDue" && rule.minutes === minutes)}
        label={minutes === 0 ? "计划时刻" : minutes < 60 ? `提前 ${minutes} 分钟` : `提前 ${minutes / 60} 小时`}
        onSelect={() => addPresetReminder(minutes)}
      />
    {/each}
    <MenuSeparator />
    <MenuItem icon={Bell} label="自定义时刻…">
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
      <div slot="submenu" class="habit-custom-col" on:click|stopPropagation>
        <input type="date" bind:value={reminderDate} on:keydown|stopPropagation />
        <input type="time" bind:value={reminderTime} on:keydown|stopPropagation />
        <button type="button" class="menu-action-button" on:click={addCustomReminder}><Check size={14} />添加提醒</button>
      </div>
    </MenuItem>
  </ContextMenu>
{/if}

{#if iconPickerOpen}
  {#await import("../../IconPicker.svelte")}
    <p class="toolbox-empty">正在打开图标目录…</p>
  {:then module}
    <svelte:component
      this={module.default}
      mode="icon"
      selected={icon}
      onPick={(picked) => { icon = picked; iconPickerOpen = false; }}
      onClose={() => (iconPickerOpen = false)}
    />
  {:catch}
    <p class="toolbox-empty">图标目录打开失败</p>
  {/await}
{/if}
</div>