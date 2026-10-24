<script lang="ts">
  /**
   * 日程事件的右键菜单（v0.9.0）：编辑 / 转到关联条目 / 删除。
   * 与日记的 DiaryEntryMenu 同一套 ContextMenu + MenuItem 结构。
   */
  import { createEventDispatcher } from "svelte";
  import { ListTodo, PenLine, Trash2 } from "@lucide/svelte";
  import ContextMenu from "../menu/ContextMenu.svelte";
  import MenuItem from "../menu/MenuItem.svelte";
  import MenuSeparator from "../menu/MenuSeparator.svelte";
  import { deleteCalendarEvent } from "../actions";
  import { showToast } from "../stores";
  import type { CalendarEvent } from "../types";

  export let x = 0;
  export let y = 0;
  export let event: CalendarEvent;

  const dispatch = createEventDispatcher<{ edit: string; gotoTask: string; close: void }>();

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

  function remove(): void {
    close();
    void deleteCalendarEvent(event.id).then(() => showToast("日程已删除"));
  }
</script>

<ContextMenu {x} {y} minWidth={200} onClose={close}>
  <MenuItem icon={PenLine} label="编辑日程" onSelect={edit} />
  {#if event.taskId}
    <MenuItem icon={ListTodo} label="转到关联条目" onSelect={gotoTask} />
  {/if}
  <MenuSeparator />
  <MenuItem icon={Trash2} danger label="删除" onSelect={remove} />
</ContextMenu>
