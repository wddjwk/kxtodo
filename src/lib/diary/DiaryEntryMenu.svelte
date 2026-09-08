<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { CalendarDays, CloudSun, PenLine, Plus, Smile, Tag as TagIcon, Trash2 } from "@lucide/svelte";
  import { deleteDiaryEntry, updateDiaryEntry } from "../actions";
  import DatePicker from "../DatePicker.svelte";
  import ContextMenu from "../menu/ContextMenu.svelte";
  import MenuItem from "../menu/MenuItem.svelte";
  import MenuSeparator from "../menu/MenuSeparator.svelte";
  import { MOOD_PRESETS, WEATHER_PRESETS } from "../diary";
  import type { DiaryEntry, Tag, TagColor } from "../types";

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

  const TAG_COLORS: Array<[TagColor, string]> = [
    ["red", "红色"],
    ["yellow", "黄色"],
    ["blue", "蓝色"],
    ["green", "绿色"],
    ["gray", "灰色"]
  ];

  let tagInputText = "";
  let selectedTagColor: TagColor = "yellow";
  let editingTagId = "";
  let editingTagText = "";

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

  // ---- 标签：整体替换（与任务菜单同一套交互，写入仍过命令层） ----
  function withTags(next: Tag[]): void {
    void updateDiaryEntry(entry.id, { tags: next });
  }

  function submitTagInput(): void {
    const text = tagInputText.trim().slice(0, 20);
    if (!text) return;
    withTags([
      ...entry.tags,
      { id: `tag-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`, color: selectedTagColor, text }
    ]);
    tagInputText = "";
  }

  function submitTagEdit(): void {
    if (!editingTagId) return;
    withTags(
      entry.tags.map((tag) =>
        tag.id === editingTagId ? { ...tag, text: editingTagText.trim().slice(0, 20) || undefined } : tag
      )
    );
    editingTagId = "";
  }

  function removeTag(tagId: string): void {
    withTags(entry.tags.filter((tag) => tag.id !== tagId));
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
  <MenuItem icon={TagIcon} label="标签">
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div slot="submenu" class="tag-editor-panel" on:click|stopPropagation={() => (editingTagId = "")}>
      {#if entry.tags.length > 0}
        {#each entry.tags as tag (tag.id)}
          {#if editingTagId === tag.id}
            <div class="tag-editor-input-row" on:click|stopPropagation>
              <input
                type="text"
                maxlength="20"
                value={editingTagText}
                on:input={(e) => (editingTagText = e.currentTarget.value)}
                on:keydown|stopPropagation={(e) => { if (e.key === "Enter" && !e.isComposing && e.keyCode !== 229) submitTagEdit(); }}
                on:blur={submitTagEdit}
              />
              <button class="tag-add-btn" type="button" on:click|stopPropagation={submitTagEdit}>
                <Plus size={15} />
              </button>
            </div>
          {:else}
            <div
              class={`tag-list-item bg-${tag.color}`}
              on:click|stopPropagation={() => { editingTagId = tag.id; editingTagText = tag.text || ""; }}
            >
              <span class="tag-list-text">{tag.text || "(无文字)"}</span>
              <button class="tag-list-delete" type="button" title="删除此标签" on:click|stopPropagation={() => removeTag(tag.id)}>
                <Trash2 size={14} />
              </button>
            </div>
          {/if}
        {/each}
      {/if}
      <div class="tag-editor-input-row">
        <input
          type="text"
          placeholder="输入标签文字..."
          maxlength="20"
          value={tagInputText}
          on:input={(e) => (tagInputText = e.currentTarget.value)}
          on:keydown|stopPropagation={(e) => { if (e.key === "Enter" && !e.isComposing && e.keyCode !== 229) submitTagInput(); }}
        />
        <button class="tag-add-btn" type="button" title="添加标签" on:click|stopPropagation={submitTagInput}>
          <Plus size={15} />
        </button>
      </div>
      <div class="tag-editor-colors">
        {#each TAG_COLORS as [color, label]}
          <button
            class={`color-circle ${color}`}
            class:selected={selectedTagColor === color}
            title={label}
            on:click|stopPropagation={() => (selectedTagColor = color)}
          ></button>
        {/each}
      </div>
      {#if entry.tags.length > 0}
        <button class="menu-item menu-item-button danger tag-clear-all" on:click|stopPropagation={() => withTags([])}>
          <Trash2 size={14} /> 清除所有标签
        </button>
      {/if}
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
