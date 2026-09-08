<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { CalendarDays, CloudSun, PenLine, Smile, Trash2 } from "@lucide/svelte";
  import { deleteDiaryEntry, updateDiaryEntry } from "../actions";
  import DatePicker from "../DatePicker.svelte";
  import ContextMenu from "../menu/ContextMenu.svelte";
  import MenuItem from "../menu/MenuItem.svelte";
  import MenuSeparator from "../menu/MenuSeparator.svelte";
  import { MOOD_PRESETS, WEATHER_PRESETS } from "../diary";
  import type { DiaryEntry } from "../types";

  /**
   * 日记卡片菜单。日记视图与全局搜索结果都要用同一份，所以抽出来——
   * 两处各写一遍的话，改一个动作就会漏掉另一个入口。
   */
  export let x = 0;
  export let y = 0;
  export let entry: DiaryEntry;
  /** 相对哪一天算「今天」（清除日期时回落到它） */
  export let today = "";

  const dispatch = createEventDispatcher<{ edit: string; close: void }>();

  function close(): void {
    dispatch("close");
  }

  function edit(): void {
    dispatch("edit", entry.id);
  }

  function setDate(date: string): void {
    close();
    void updateDiaryEntry(entry.id, { date });
  }

  function setMood(emoji: string): void {
    close();
    void updateDiaryEntry(entry.id, { mood: emoji });
  }

  function setWeather(emoji: string): void {
    close();
    void updateDiaryEntry(entry.id, { weather: emoji });
  }

  function remove(): void {
    close();
    void deleteDiaryEntry(entry.id);
  }
</script>

<ContextMenu {x} {y} minWidth={216} onClose={close}>
  <MenuItem icon={PenLine} label="编辑" onSelect={edit} />
  <MenuItem icon={CalendarDays} label="修改日期">
    <div slot="submenu" class="task-menu-date">
      <DatePicker value={entry.date} on:select={(event) => setDate(event.detail)} on:clear={() => setDate(today)} />
    </div>
  </MenuItem>
  <MenuItem icon={Smile} label="心情">
    <div slot="submenu" class="diary-emoji-menu">
      {#each MOOD_PRESETS as preset (preset.emoji)}
        <button
          class="emoji-pick-cell"
          type="button"
          class:selected={entry.mood === preset.emoji}
          title={preset.label}
          on:click|stopPropagation={() => setMood(preset.emoji)}
        >{preset.emoji}</button>
      {/each}
    </div>
  </MenuItem>
  <MenuItem icon={CloudSun} label="天气">
    <div slot="submenu" class="diary-emoji-menu">
      {#each WEATHER_PRESETS as preset (preset.emoji)}
        <button
          class="emoji-pick-cell"
          type="button"
          class:selected={entry.weather === preset.emoji}
          title={preset.label}
          on:click|stopPropagation={() => setWeather(preset.emoji)}
        >{preset.emoji}</button>
      {/each}
    </div>
  </MenuItem>
  <MenuSeparator />
  <MenuItem icon={Trash2} danger label="删除" onSelect={remove} />
</ContextMenu>
