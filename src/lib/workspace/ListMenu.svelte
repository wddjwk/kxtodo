<script lang="ts">
  import ContextMenu from "../menu/ContextMenu.svelte";
  import MenuItem from "../menu/MenuItem.svelte";
  import MenuSeparator from "../menu/MenuSeparator.svelte";
  import { ArrowUpDown, Download, Eraser, FolderInput, Image, PenLine, RotateCcw, Trash2, Upload } from "@lucide/svelte";
  import { appSettings, appState, selectedBackground, accent, showToast, now, safeFileName, fileToDataUrl, appVersion } from "../stores";
  import {
    deleteNodeCascade as deleteNodeCascadeAction,
    importState as importStateAction,
    setBackground as setBackgroundAction,
    setConfig as setConfigAction,
    setUiColor as setUiColorAction,
    unsetUiColor as unsetUiColorAction,
    applyTreeOrder as applyTreeOrderAction
  } from "../actions";
  import { moveTargetOptions, nodeAndDescendantIds, exportStateForNode } from "../nodes";
  import { normalizeState, normalizeSettings, defaultBackground, themePresets } from "../defaults";
  import {
    exportData, isTauriRuntime, deleteBackgroundImage, pickImageFile,
    importBackgroundImage, backgroundImageUrl, deleteNodeImages
  } from "../backend";
  import { isLocalImageRef, localImageFilename, localImageRef, primeImageCache } from "../images";
  import { showMobileList } from "../platform";
  import { sortLabels, type SortMode } from "../sort";
  import type { AppNode, ListBackground } from "../types";

  export let x = 0;
  export let y = 0;
  export let node: AppNode | undefined = undefined;
  export let isScheduled = false;
  export let sortMode: SortMode = "created-desc";
  export let onSortMode: (mode: SortMode) => void = () => {};
  export let onRenameRequest: () => void = () => {};
  export let onClose: () => void = () => {};

  let importInput: HTMLInputElement;
  let colorPickerInput: HTMLInputElement;
  let backgroundFileInput: HTMLInputElement;
  let editingPresetIndex: number | null = null;
  let presetNameDraft = "";
  let presetColorDraft = "";
  let presetEditOriginalColor = "";

  $: isSystemNode = !node || node.kind === "system";
  $: moveTargets = node ? moveTargetOptions(node.id, $appState.nodes) : [];
  $: presets = $appSettings.appearance.themePresets.length
    ? $appSettings.appearance.themePresets
    : themePresets;
  $: backgroundLinkDraft = isLocalImageRef($selectedBackground.image) ? "" : ($selectedBackground.image ?? "");

  function setBackground(patch: Partial<ListBackground>): void {
    if (!node) return;
    void setBackgroundAction(node.id, {
      color: patch.color,
      image: patch.image === undefined ? undefined : patch.image ?? null,
      imageOpacity: patch.imageOpacity
    });
  }

  function applyTheme(color: string): void {
    setBackground({ color });
  }

  function handleColorPick(event: Event): void {
    const target = event.currentTarget;
    if (target instanceof HTMLInputElement) {
      applyTheme(target.value);
    }
  }

  function openColorPicker(): void {
    colorPickerInput?.click();
  }

  function updateBackgroundLink(event: Event): void {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement)) return;
    const previous = $selectedBackground.image;
    const next = target.value.trim() || undefined;
    setBackground({ image: next });
    if (isLocalImageRef(previous) && previous !== next) void deleteBackgroundImage(localImageFilename(previous));
  }

  function updateBackgroundOpacity(event: Event): void {
    const target = event.currentTarget;
    if (target instanceof HTMLInputElement) {
      setBackground({ imageOpacity: Number(target.value) / 100 });
    }
  }

  async function pickBackgroundImage(): Promise<void> {
    if (!isTauriRuntime) {
      backgroundFileInput.click();
      return;
    }
    try {
      const path = await pickImageFile();
      if (!path) return;
      const previous = $selectedBackground.image;
      const filename = await importBackgroundImage(path);
      const url = await backgroundImageUrl(filename);
      primeImageCache(filename, url);
      setBackground({ image: localImageRef(filename) });
      if (isLocalImageRef(previous)) void deleteBackgroundImage(localImageFilename(previous));
    } catch (error) {
      showToast(`背景图片读取失败：${String(error)}`);
    }
  }

  async function uploadBackgroundImage(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) return;
    try {
      const dataUrl = await fileToDataUrl(target.files[0]);
      setBackground({ image: dataUrl });
    } catch (error) {
      showToast(`背景图片读取失败：${String(error)}`);
    } finally {
      target.value = "";
    }
  }

  function clearBackground(): void {
    const previous = $selectedBackground.image;
    setBackground({ image: undefined });
    if (isLocalImageRef(previous)) void deleteBackgroundImage(localImageFilename(previous));
  }

  function setUiColor(color: string): void {
    if (!node) return;
    void setUiColorAction(node.id, color);
  }

  function handleUiColorPick(event: Event): void {
    const target = event.currentTarget;
    if (target instanceof HTMLInputElement) {
      setUiColor(target.value);
    }
  }

  function resetUiColor(): void {
    if (!node) return;
    void unsetUiColorAction(node.id);
  }

  function resetBackgroundToDefault(): void {
    void setConfigAction("appearance.themePresets", themePresets.map((preset) => ({ ...preset })));
    setBackground({ color: defaultBackground.color });
  }

  function beginPresetEdit(index: number): void {
    const preset = presets[index];
    if (!preset) return;
    editingPresetIndex = index;
    presetNameDraft = preset.name;
    presetColorDraft = preset.color;
    presetEditOriginalColor = $selectedBackground.color;
  }

  function cancelPresetEdit(): void {
    if (editingPresetIndex !== null && presetEditOriginalColor) {
      setBackground({ color: presetEditOriginalColor });
    }
    editingPresetIndex = null;
    presetNameDraft = "";
    presetColorDraft = "";
    presetEditOriginalColor = "";
  }

  function normalizeHexColor(value: string, fallback: string): string {
    const color = value.trim();
    return /^#[0-9a-f]{6}$/i.test(color) ? color : fallback;
  }

  function updatePresetName(event: Event): void {
    const target = event.currentTarget;
    if (target instanceof HTMLInputElement) {
      presetNameDraft = target.value;
    }
  }

  function updatePresetColor(event: Event): void {
    const target = event.currentTarget;
    if (target instanceof HTMLInputElement) {
      presetColorDraft = target.value;
      const validColor = normalizeHexColor(target.value, "");
      if (validColor) {
        setBackground({ color: validColor });
      }
    }
  }

  function savePresetEdit(): void {
    if (editingPresetIndex === null) return;
    const nextPresets = presets.map((preset) => ({ ...preset }));
    const current = nextPresets[editingPresetIndex];
    if (!current) return;
    const finalColor = normalizeHexColor(presetColorDraft, current.color);
    nextPresets[editingPresetIndex] = {
      name: presetNameDraft.trim().slice(0, 24) || current.name,
      color: finalColor
    };
    void setConfigAction("appearance.themePresets", nextPresets);
    setBackground({ color: finalColor });
    cancelPresetEditOnly();
  }

  function cancelPresetEditOnly(): void {
    editingPresetIndex = null;
    presetNameDraft = "";
    presetColorDraft = "";
    presetEditOriginalColor = "";
  }

  function deleteCurrentNode(): void {
    if (!node || node.kind === "system") {
      showToast("内置列表不能删除");
      return;
    }
    const id = node.id;
    const ids = nodeAndDescendantIds(id, $appState.nodes);
    for (const delId of ids) {
      const bg = $appState.backgrounds[delId];
      if (bg?.image && isLocalImageRef(bg.image)) {
        void deleteBackgroundImage(localImageFilename(bg.image));
      }
      void deleteNodeImages(delId);
    }
    void deleteNodeCascadeAction(id);
    onClose();
    showMobileList();
  }

  async function exportCurrentList(): Promise<void> {
    if (!node) return;
    const payload = {
      version: $appVersion || "0.0.0",
      exportedAt: now(),
      scope: "node",
      nodeId: node.id,
      state: exportStateForNode(node, $appState)
    };
    await exportData(payload, `${safeFileName(node.name)}-${$appVersion || "dev"}.json`);
    onClose();
    showToast("导出完成");
  }

  async function exportAll(): Promise<void> {
    const payload = {
      version: $appVersion || "0.0.0",
      exportedAt: now(),
      scope: "all",
      state: $appState,
      settings: $appSettings
    };
    await exportData(payload, `kxtodo-${$appVersion || "dev"}-all.json`);
    onClose();
    showToast("全部数据已导出");
  }

  async function importFromFile(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) return;
    try {
      const payload = JSON.parse(await target.files[0].text()) as { state?: unknown; settings?: unknown };
      const normalizedState = normalizeState(payload.state ?? payload);
      await importStateAction(
        normalizedState,
        payload.settings ? normalizeSettings(payload.settings) : null
      );
    } catch (error) {
      showToast(`导入失败：${String(error)}`);
    } finally {
      target.value = "";
      onClose();
    }
  }

  function moveNodeToGroup(nodeId: string, parentId: string | null): void {
    const source = $appState.nodes.find((n) => n.id === nodeId);
    if (!source || source.kind === "system" || source.parentId === parentId) {
      onClose();
      return;
    }
    const targetParent = parentId ? $appState.nodes.find((n) => n.id === parentId && n.kind === "category") : null;
    if (parentId && !targetParent) {
      showToast("目标分组不存在");
      return;
    }
    if (source.kind === "category" && parentId && nodeAndDescendantIds(source.id, $appState.nodes).has(parentId)) {
      showToast("不能移动到自身或自己的子分类中");
      return;
    }
    const withoutSource = $appState.nodes.filter((n) => n.id !== nodeId);
    const sourceWithParent = { ...source, parentId };
    let insertIndex = withoutSource.length;
    if (parentId) {
      const siblingIndexes = withoutSource.map((n, i) => ({ n, i })).filter((item) => item.n.parentId === parentId).map((item) => item.i);
      const parentIndex = withoutSource.findIndex((n) => n.id === parentId);
      insertIndex = siblingIndexes.length ? Math.max(...siblingIndexes) + 1 : parentIndex >= 0 ? parentIndex + 1 : withoutSource.length;
    }
    const nodes = [...withoutSource];
    nodes.splice(insertIndex, 0, sourceWithParent);
    const ordered = nodes.map((n) => (parentId && n.id === parentId ? { ...n, collapsed: false } : n));
    void applyTreeOrderAction(ordered, { [nodeId]: parentId });
    onClose();
  }
</script>

<ContextMenu {x} {y} minWidth={300} {onClose}>
  {#if !isSystemNode && node}
    <MenuItem icon={PenLine} label="重命名" onSelect={() => { onRenameRequest(); }} />
    <MenuItem icon={FolderInput} label="移动到分组">
      <div slot="submenu" class="submenu-list">
        {#each moveTargets as target (target.id)}
          <MenuItem
            label={target.name}
            active={(node.parentId ?? "") === target.id}
            onSelect={() => moveNodeToGroup(node.id, target.id || null)}
          />
        {:else}
          <div class="menu-empty">没有可移动的目标</div>
        {/each}
      </div>
    </MenuItem>
  {/if}
  {#if !isScheduled}
    <MenuItem icon={ArrowUpDown} label="排序方式">
      <div slot="submenu" class="submenu-list">
        {#each Object.entries(sortLabels) as [mode, label]}
          <MenuItem
            label={label as string}
            active={sortMode === mode}
            onSelect={() => { onSortMode(mode as SortMode); onClose(); }}
          />
        {/each}
      </div>
    </MenuItem>
  {/if}
  {#if !isSystemNode}
    <MenuItem icon={Trash2} danger label="删除当前条目" onSelect={deleteCurrentNode} />
  {/if}

  <MenuSeparator />
  <MenuItem icon={Upload} label="导出当前" onSelect={() => void exportCurrentList()} />
  <MenuItem icon={Upload} label="一键全部导出" onSelect={() => void exportAll()} />
  <MenuItem icon={Download} label="导入 JSON" onSelect={() => importInput.click()} />

  <MenuSeparator />
  <div class="menu-section-title">UI颜色</div>
  <div class="ui-color-row">
    <label class="ui-color-picker" title="修改当前界面的标题和控件颜色">
      <span style={`--swatch: ${$accent}`}></span>
      <input type="color" value={$accent} on:input={handleUiColorPick} />
    </label>
    <span class="ui-color-value">{$accent}</span>
    <button class="menu-action-button" type="button" on:click={resetUiColor}>默认</button>
  </div>

  <div class="menu-section-title">背景颜色</div>
  <div class="color-grid">
    {#each presets as preset, index (preset.name + index)}
      <button
        type="button"
        title={`${preset.name}（右键编辑）`}
        class:editing={editingPresetIndex === index}
        style={`--swatch: ${preset.color}; --accent-color: ${preset.color}`}
        on:click={() => applyTheme(preset.color)}
        on:contextmenu|preventDefault|stopPropagation={() => beginPresetEdit(index)}
      ></button>
    {/each}
    <button type="button" class="palette-button" title="自定义颜色" on:click={openColorPicker}></button>
    <button type="button" class="reset-bg-button" title="恢复默认配色" on:click={resetBackgroundToDefault}>
      <RotateCcw size={14} />
    </button>
  </div>
  {#if editingPresetIndex !== null}
    <div class="preset-editor">
      <div class="preset-editor-title">编辑预设颜色</div>
      <input value={presetNameDraft} maxlength="24" placeholder="颜色名称" on:input={updatePresetName} />
      <div class="preset-color-line">
        <input type="color" value={presetColorDraft} on:input={updatePresetColor} />
        <input value={presetColorDraft} placeholder="#dfe8df" on:input={updatePresetColor} />
      </div>
      <div class="preset-editor-actions">
        <button type="button" on:click={savePresetEdit}>保存</button>
        <button type="button" on:click={cancelPresetEdit}>取消</button>
      </div>
    </div>
  {/if}
  <input bind:this={colorPickerInput} class="hidden-file" type="color" value={$selectedBackground.color} on:input={handleColorPick} />
  <label class="background-link">
    背景图片链接
    <input value={backgroundLinkDraft} placeholder="https://..." on:input={updateBackgroundLink} />
  </label>
  <label class="opacity-row">
    图片透明度
    <input type="range" min="0" max="80" value={Math.round(($selectedBackground.imageOpacity ?? 0.28) * 100)} on:input={updateBackgroundOpacity} />
  </label>
  <div class="menu-inline two">
    <button class="menu-action-button" type="button" on:click={pickBackgroundImage}><Image size={15} /> 上传图片</button>
    <button class="menu-action-button" type="button" on:click={clearBackground}><Eraser size={15} /> 清除背景</button>
  </div>

  <input bind:this={importInput} class="hidden-file" type="file" accept="application/json,.json" on:change={importFromFile} />
  <input bind:this={backgroundFileInput} class="hidden-file" type="file" accept="image/*" on:change={uploadBackgroundImage} />
</ContextMenu>
