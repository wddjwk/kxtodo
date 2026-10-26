<script lang="ts">
  /**
   * 日程事件的右键菜单（v0.9.0；v0.9.1 加重复日程的三档删除）。
   * 与日记的 DiaryEntryMenu 同一套 ContextMenu + MenuItem 结构。
   *
   * 重复日程删除的三档就是业界口径：只删这一次（写进例外日期）/ 删此次及以后
   * （把重复截止日期收到它前一天）/ 删整个系列。非重复日程只有「删除」。
   */
  import { createEventDispatcher } from "svelte";
  import { CalendarX2, ListTodo, PenLine, Scissors, Trash2 } from "@lucide/svelte";
  import ContextMenu from "../menu/ContextMenu.svelte";
  import MenuItem from "../menu/MenuItem.svelte";
  import MenuSeparator from "../menu/MenuSeparator.svelte";
  import { deleteCalendarEvent } from "../actions";
  import { showToast } from "../stores";
  import type { CalendarEvent } from "../types";

  export let x = 0;
  export let y = 0;
  export let event: CalendarEvent;
  /** 右键的是重复日程的哪一次（非重复事件 = 开始日期） */
  export let occurrence = "";

  const dispatch = createEventDispatcher<{ edit: string; gotoTask: string; close: void }>();

  $: recurring = event.repeat !== null && event.repeat !== undefined;
  $: at = occurrence || event.startDate;

  function close(): void {
    dispatch("close");
  }

  function edit(): void {
    dispatch("edit", event.id);
  }

  function gotoTask(): void {
    if (!event.taskId) return;
    dispatch("gotoTask", event.taskId);
  }

  async function remove(scope?: { occurrence?: string; fromOccurrence?: string }): Promise<void> {
    close();
    await deleteCalendarEvent(event.id, scope);
    showToast(
      scope?.occurrence
        ? `已删除 ${scope.occurrence} 这一次`
        : scope?.fromOccurrence
          ? `已删除 ${scope.fromOccurrence} 起的后续全部`
          : "日程已删除"
    );
  }
</script>

<ContextMenu {x} {y} minWidth={210} onClose={close}>
  <MenuItem icon={PenLine} label="编辑日程" onSelect={edit} />
  {#if event.taskId}
    <MenuItem icon={ListTodo} label="转到关联条目" onSelect={gotoTask} />
  {/if}
  <MenuSeparator />
  {#if recurring}
    <MenuItem
      icon={CalendarX2}
      danger
      label={`只删 ${at} 这一次`}
      onSelect={() => void remove({ occurrence: at })}
    />
    <MenuItem
      icon={Scissors}
      danger
      label={at <= event.startDate ? "删除此次及以后（= 整个日程）" : "删除此次及以后"}
      onSelect={() => void remove({ fromOccurrence: at })}
    />
    <MenuSeparator />
    <MenuItem icon={Trash2} danger label="删除整个日程" onSelect={() => void remove()} />
  {:else}
    <MenuItem icon={Trash2} danger label="删除" onSelect={() => void remove()} />
  {/if}
</ContextMenu>
