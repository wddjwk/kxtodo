<script lang="ts">
  import { get } from "svelte/store";
  import ContextMenu from "../menu/ContextMenu.svelte";
  import MenuItem from "../menu/MenuItem.svelte";
  import MenuSeparator from "../menu/MenuSeparator.svelte";
  import { ArrowUpDown, CalendarRange, Download, Eraser, Eye, EyeOff, FileArchive, FolderInput, Image, LayoutGrid, ListTodo, PenLine, RefreshCw, RotateCcw, Trash2, Upload } from "@lucide/svelte";
  import { appSettings, appState, selectedBackground, accent, showToast, now, safeFileName, fileToDataUrl, appVersion } from "../stores";
  import {
    deleteNodeCascade as deleteNodeCascadeAction,
    importState as importStateAction,
    setBackground as setBackgroundAction,
    setConfig as setConfigAction,
    setNodeCardStyle as setNodeCardStyleAction,
    setUiColor as setUiColorAction,
    unsetUiColor as unsetUiColorAction,
    applyTreeOrder as applyTreeOrderAction,
    exportDiaryArchive as exportDiaryArchiveAction,
    importDiaryArchive as importDiaryArchiveAction,
    importDiaryArchiveFile as importDiaryArchiveFileAction,
    exportLedgerArchive as exportLedgerArchiveAction,
    importLedgerArchive as importLedgerArchiveAction,
    importLedgerArchiveFile as importLedgerArchiveFileAction,
    exportCardsArchive as exportCardsArchiveAction,
    importCardsArchive as importCardsArchiveAction,
    importCardsArchiveFile as importCardsArchiveFileAction,
    syncNow as syncNowAction
  } from "../actions";
  import { moveTargetOptions, nodeAndDescendantIds, exportStateForNode } from "../nodes";
  import { normalizeState, normalizeSettings, defaultBackground, themePresets } from "../defaults";
  import {
    exportData, isTauriRuntime, deleteBackgroundImage, pickImageFile,
    importBackgroundImage, backgroundImageUrl, deleteNodeImages, saveBackgroundImageFromDataUrl
  } from "../backend";
  import { caps } from "../capabilities";
  import { isLocalImageRef, localImageFilename, localImageRef, primeImageCache } from "../images";
  import { showMobileList } from "../platform";
  import { sortLabels, type SortMode } from "../sort";
  import type { AppNode, ListBackground } from "../types";

  export let x = 0;
  export let y = 0;
  export let xAlign: "left" | "right" = "left";
  export let node: AppNode | undefined = undefined;
  export let isScheduled = false;
  /** 计划内视图专属：是否显示"已完成"任务（父组件持有状态）。 */
  export let isPlanned = false;
  export let showCompleted = true;
  export let onToggleShowCompleted: () => void = () => {};
  export let sortMode: SortMode = "created-desc";
  export let onSortMode: (mode: SortMode) => void = () => {};
  export let onRenameRequest: () => void = () => {};
  export let onClose: () => void = () => {};
  /**
   * 日记模式：日记不是节点，背景与主题色来自 `settings.diary`（由调用方以
   * `background` / `accentColor` 覆盖进来），导出导入走 zip 而不是 JSON。
   */
  export let diaryMode = false;
  /** 记账模式：与日记同一条套路——外观在 settings.ledger，导出导入走 Excel 压缩包。 */
  export let ledgerMode = false;
  export let background: ListBackground | null = null;
  export let accentColor: string | null = null;

  let importInput: HTMLInputElement;
  let colorPickerInput: HTMLInputElement;
  let backgroundFileInput: HTMLInputElement;
  let editingPresetIndex: number | null = null;
  let presetNameDraft = "";
  let presetColorDraft = "";
  let presetEditOriginalColor = "";
  let syncing = false;

  /**
   * 拖动期间的本地草稿。日记模式的写入走 `config.set`——它等 IPC 往返回来才更新
   * store，滞后的回渲会把「已提交的旧值」写回 range/color input，thumb 被拽回去
   * （透明度条不跟手、取色器跳变的根因；条目页的 setBackground 同步改 store 所以没这病）。
   * 交互期间显示值冻结、不跟随 committed，change/blur 后再放开。
   */
  let opacityLive = false;
  let opacityValue = 0;
  let uiColorLive = false;
  let uiColorValue = "";
  let linkLive = false;
  let linkValue = "";

  $: opacityCommitted = Math.round((bg.imageOpacity ?? defaultBackground.imageOpacity ?? 0.28) * 100);
  $: if (!opacityLive) opacityValue = opacityCommitted;
  $: if (!uiColorLive) uiColorValue = accentValue;
  $: linkCommitted = isLocalImageRef(bg.image) ? "" : (bg.image ?? "");
  $: if (!linkLive) linkValue = linkCommitted;

  /** 同步已配对且没暂停才给「立即同步」入口（与设置页/下拉同一口径，总开关关掉一律不给） */
  $: syncReady =
    $appSettings.features?.sync !== false &&
    Boolean($appSettings.sync?.enabled) &&
    Boolean(($appSettings.sync?.username ?? "").trim()) &&
    Boolean(($appSettings.sync?.secret ?? "").trim());

  async function runSync(): Promise<void> {
    if (syncing) return;
    syncing = true;
    try {
      await syncNowAction();
    } finally {
      syncing = false;
    }
  }

  /**
   * 隐藏 file input 拿不到焦点：系统文件选择器打开 → 窗口 blur → ContextMenu
   * 的 blur-close 把菜单连同 input 一起卸载 → change 事件永远丢失（移动端
   * “上传背景后没反应”的根因）。选择器打开期间吞掉 onClose，窗口重新聚焦后
   * 延时复位（覆盖用户取消选择、不触发 change 的路径）。
   */
  let filePickerOpen = false;
  let filePickerResetTimer: number | undefined;

  function markFilePickerOpen(): void {
    filePickerOpen = true;
    window.clearTimeout(filePickerResetTimer);
    window.addEventListener("focus", scheduleFilePickerReset, { once: true });
    // 兜底：选择器若始终不产生 blur/focus 回合（WebView 差异或 click 失败），
    // 30s 后强制复位，避免菜单永久吞掉所有关闭路径。
    filePickerResetTimer = window.setTimeout(() => {
      filePickerOpen = false;
    }, 30_000);
  }

  function scheduleFilePickerReset(): void {
    window.clearTimeout(filePickerResetTimer);
    filePickerResetTimer = window.setTimeout(() => {
      filePickerOpen = false;
    }, 600);
  }

  function handleClose(): void {
    if (filePickerOpen) return;
    onClose();
  }

  $: isSystemNode = !node || node.kind === "system";
  $: moveTargets = node ? moveTargetOptions(node.id, $appState.nodes) : [];
  $: presets = $appSettings.appearance.themePresets.length
    ? $appSettings.appearance.themePresets
    : themePresets;
  /** 生效的背景与主题色：日记模式用传进来的覆盖值，否则跟着当前选中的条目 */
  $: bg = background ?? $selectedBackground;
  $: accentValue = accentColor ?? $accent;
  /** 日记/记账模式的外观写在 settings 的扁平配置项上，前缀不同其余同构 */
  $: settingsPrefix = diaryMode ? "diary" : ledgerMode ? "ledger" : "";

  function setBackground(patch: Partial<ListBackground>): void {
    if (settingsPrefix) {
      // 日记/记账的外观是 settings.<域> 上的几个扁平配置项，一个 patch 拆成多次 config.set
      if (patch.color !== undefined) void setConfigAction(`${settingsPrefix}.backgroundColor`, patch.color);
      if (patch.image !== undefined) {
        void setConfigAction(`${settingsPrefix}.backgroundImage`, patch.image ?? "");
      }
      if (patch.imageOpacity !== undefined) {
        void setConfigAction(`${settingsPrefix}.backgroundOpacity`, patch.imageOpacity);
      }
      return;
    }
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
    linkLive = true;
    linkValue = target.value;
    const previous = bg.image;
    const next = target.value.trim() || undefined;
    setBackground({ image: next });
    if (isLocalImageRef(previous) && previous !== next) void deleteBackgroundImage(localImageFilename(previous));
  }

  function endBackgroundLinkEdit(): void {
    linkLive = false;
  }

  function updateBackgroundOpacity(event: Event): void {
    const target = event.currentTarget;
    if (target instanceof HTMLInputElement) {
      opacityLive = true;
      opacityValue = Number(target.value);
      setBackground({ imageOpacity: Number(target.value) / 100 });
    }
  }

  function endBackgroundOpacityEdit(): void {
    opacityLive = false;
  }

  async function pickBackgroundImage(): Promise<void> {
    // 浏览器与移动端（无原生对话框）都走隐藏 <input type=file> → uploadBackgroundImage。
    if (!isTauriRuntime || !caps.nativeFileDialogs) {
      markFilePickerOpen();
      backgroundFileInput.click();
      return;
    }
    try {
      const path = await pickImageFile();
      if (!path) return;
      const previous = bg.image;
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
      if (isTauriRuntime) {
        // Tauri（移动端 + 桌面兜底）：dataURL 交给 Rust 落盘为本地图片文件。
        const previous = bg.image;
        const filename = await saveBackgroundImageFromDataUrl(dataUrl);
        const url = await backgroundImageUrl(filename);
        primeImageCache(filename, url);
        setBackground({ image: localImageRef(filename) });
        if (isLocalImageRef(previous)) void deleteBackgroundImage(localImageFilename(previous));
      } else {
        setBackground({ image: dataUrl });
      }
    } catch (error) {
      showToast(`背景图片读取失败：${String(error)}`);
    } finally {
      target.value = "";
      window.clearTimeout(filePickerResetTimer);
      filePickerOpen = false;
    }
  }

  /** 清除背景 = 恢复默认：必须显式传 image: null（undefined 会被 actions.setBackground
   * 视为“不修改”，沿用旧图片导致清除无效），颜色与透明度一并回默认值。 */
  async function clearBackground(): Promise<void> {
    if (settingsPrefix) {
      const previous = bg.image;
      await Promise.all([
        setConfigAction(`${settingsPrefix}.backgroundColor`, defaultBackground.color),
        setConfigAction(`${settingsPrefix}.backgroundImage`, ""),
        setConfigAction(`${settingsPrefix}.backgroundOpacity`, defaultBackground.imageOpacity ?? 0.28)
      ]);
      if (isLocalImageRef(previous)) {
        void deleteBackgroundImage(localImageFilename(previous));
      }
      return;
    }
    if (!node) return;
    const previous = $selectedBackground.image;
    await setBackgroundAction(node.id, {
      color: defaultBackground.color,
      image: null,
      imageOpacity: defaultBackground.imageOpacity
    });
    // 写入确认生效后再删旧文件，避免存储条目指向已删除的本地图片
    if (isLocalImageRef(previous) && !get(appState).backgrounds[node.id]?.image) {
      void deleteBackgroundImage(localImageFilename(previous));
    }
  }

  function setUiColor(color: string): void {
    if (settingsPrefix) {
      void setConfigAction(`${settingsPrefix}.accent`, color);
      return;
    }
    if (!node) return;
    void setUiColorAction(node.id, color);
  }

  function handleUiColorPick(event: Event): void {
    const target = event.currentTarget;
    if (target instanceof HTMLInputElement) {
      uiColorLive = true;
      uiColorValue = target.value;
      setUiColor(target.value);
    }
  }

  function endUiColorPick(): void {
    uiColorLive = false;
  }

  function resetUiColor(): void {
    if (settingsPrefix) {
      // 空串 = 用该域默认主题色（与 core 的 diary.accent / ledger.accent 同口径）
      void setConfigAction(`${settingsPrefix}.accent`, "");
      return;
    }
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
    presetEditOriginalColor = bg.color;
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
    try {
      await exportData(payload, `${safeFileName(node.name)}-${$appVersion || "dev"}.json`);
      showToast("导出完成");
    } catch (error) {
      showToast(`导出失败：${String(error)}`);
    }
    onClose();
  }

  async function exportAll(): Promise<void> {
    const payload = {
      version: $appVersion || "0.0.0",
      exportedAt: now(),
      scope: "all",
      state: $appState,
      settings: $appSettings
    };
    try {
      await exportData(payload, `kxtodo-${$appVersion || "dev"}-all.json`);
      showToast("全部数据已导出");
    } catch (error) {
      showToast(`导出失败：${String(error)}`);
    }
    onClose();
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

  // ---- 日记导出/导入（zip：年/月/YYYYMMDD[_序号][_标题].md + YAML front-matter） ----
  let diaryZipInput: HTMLInputElement;
  let cardsZipInput: HTMLInputElement;
  let exportFrom = "";
  let exportTo = "";

  /** 一键全量导出。关掉菜单再等结果：另存为/分享面板都是系统级 UI，不该压在菜单下面。 */
  async function exportAllDiary(): Promise<void> {
    onClose();
    await exportDiaryArchiveAction();
  }

  async function exportDiaryRange(): Promise<void> {
    if (!exportFrom && !exportTo) {
      showToast("先选一个起止日期");
      return;
    }
    onClose();
    await exportDiaryArchiveAction({ from: exportFrom || undefined, to: exportTo || undefined });
  }

  async function importDiary(): Promise<void> {
    if (!isTauriRuntime) {
      showToast("浏览器预览不支持导入日记压缩包");
      return;
    }
    if (!caps.nativeFileDialogs) {
      // 移动端无原生对话框：走隐藏 input，选择器打开期间必须吞掉 onClose（见 markFilePickerOpen）
      markFilePickerOpen();
      diaryZipInput.click();
      return;
    }
    onClose();
    await importDiaryArchiveAction();
  }

  async function importDiaryFromInput(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) return;
    try {
      await importDiaryArchiveFileAction(target.files[0]);
    } finally {
      target.value = "";
      window.clearTimeout(filePickerResetTimer);
      filePickerOpen = false;
      onClose();
    }
  }

  // ---- 记账导出/导入（zip 内含 kxtodo-ledger.xlsx：说明/账户/分类/账目） ----
  let ledgerZipInput: HTMLInputElement;

  async function exportAllLedger(): Promise<void> {
    onClose();
    await exportLedgerArchiveAction();
  }

  async function exportLedgerRange(): Promise<void> {
    if (!exportFrom && !exportTo) {
      showToast("先选一个起止日期");
      return;
    }
    onClose();
    await exportLedgerArchiveAction({ from: exportFrom || undefined, to: exportTo || undefined });
  }

  async function importLedger(): Promise<void> {
    if (!isTauriRuntime) {
      showToast("浏览器预览不支持导入记账压缩包");
      return;
    }
    if (!caps.nativeFileDialogs) {
      markFilePickerOpen();
      ledgerZipInput.click();
      return;
    }
    onClose();
    await importLedgerArchiveAction();
  }

  async function importLedgerFromInput(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0]) return;
    try {
      await importLedgerArchiveFileAction(target.files[0]);
    } finally {
      target.value = "";
      window.clearTimeout(filePickerResetTimer);
      filePickerOpen = false;
      onClose();
    }
  }

  // ---- 一般卡片条目的 Markdown 压缩包（一张卡片一个 md + images/） ----

  async function exportCardsMd(): Promise<void> {
    if (!node) return;
    onClose();
    await exportCardsArchiveAction(node.id);
  }

  async function importCardsMd(): Promise<void> {
    if (!node) return;
    if (!isTauriRuntime) {
      showToast("浏览器预览不支持导入 Markdown 压缩包");
      return;
    }
    if (!caps.nativeFileDialogs) {
      markFilePickerOpen();
      cardsZipInput.click();
      return;
    }
    onClose();
    await importCardsArchiveAction(node.id);
  }

  async function importCardsMdFromInput(event: Event): Promise<void> {
    const target = event.currentTarget;
    if (!(target instanceof HTMLInputElement) || !target.files?.[0] || !node) return;
    try {
      await importCardsArchiveFileAction(node.id, target.files[0]);
    } finally {
      target.value = "";
      window.clearTimeout(filePickerResetTimer);
      filePickerOpen = false;
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

<ContextMenu {x} {y} {xAlign} minWidth={300} onClose={handleClose}>
  {#if syncReady}
    <MenuItem icon={RefreshCw} label={syncing ? "同步中…" : "立即同步"} onSelect={() => { onClose(); void runSync(); }} />
  {/if}
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
  {#if !isScheduled && !diaryMode}
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
  {#if node?.kind === "entry"}
    <MenuItem icon={LayoutGrid} label="卡片类型">
      <div slot="submenu" class="submenu-list">
        <MenuItem
          icon={ListTodo}
          label="Todo卡片"
          active={(node.cardStyle ?? "todo") === "todo"}
          onSelect={() => { void setNodeCardStyleAction(node.id, "todo"); onClose(); }}
        />
        <MenuItem
          icon={LayoutGrid}
          label="一般卡片"
          active={node.cardStyle === "card"}
          onSelect={() => { void setNodeCardStyleAction(node.id, "card"); onClose(); }}
        />
      </div>
    </MenuItem>
  {/if}
  {#if isPlanned}
    <MenuItem
      icon={showCompleted ? EyeOff : Eye}
      label={showCompleted ? "隐藏已完成" : "显示已完成"}
      onSelect={() => { onToggleShowCompleted(); onClose(); }}
    />
  {/if}
  {#if !isSystemNode}
    <MenuItem icon={Trash2} danger label="删除当前条目" onSelect={deleteCurrentNode} />
  {/if}

  <MenuSeparator />
  {#if diaryMode}
    <MenuItem icon={FileArchive} label="导出全部日记" onSelect={() => void exportAllDiary()} />
    <MenuItem icon={CalendarRange} label="按日期范围导出">
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
      <div slot="submenu" class="diary-export-range" on:click|stopPropagation>
        <label>
          从
          <input type="date" bind:value={exportFrom} on:keydown|stopPropagation />
        </label>
        <label>
          到
          <input type="date" bind:value={exportTo} on:keydown|stopPropagation />
        </label>
        <button class="menu-action-button" type="button" on:click|stopPropagation={() => void exportDiaryRange()}>
          <Upload size={15} /> 导出这一段
        </button>
      </div>
    </MenuItem>
    <MenuItem icon={Download} label="导入日记压缩包" onSelect={() => void importDiary()} />
  {:else if ledgerMode}
    <MenuItem icon={FileArchive} label="导出全部账本" onSelect={() => void exportAllLedger()} />
    <MenuItem icon={CalendarRange} label="按日期范围导出">
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
      <div slot="submenu" class="diary-export-range" on:click|stopPropagation>
        <label>
          从
          <input type="date" bind:value={exportFrom} on:keydown|stopPropagation />
        </label>
        <label>
          到
          <input type="date" bind:value={exportTo} on:keydown|stopPropagation />
        </label>
        <button class="menu-action-button" type="button" on:click|stopPropagation={() => void exportLedgerRange()}>
          <Upload size={15} /> 导出这一段
        </button>
      </div>
    </MenuItem>
    <MenuItem icon={Download} label="导入记账压缩包" onSelect={() => void importLedger()} />
  {:else}
    <MenuItem icon={Upload} label="导出当前" onSelect={() => void exportCurrentList()} />
    <MenuItem icon={Upload} label="一键全部导出" onSelect={() => void exportAll()} />
    <MenuItem icon={Download} label="导入 JSON" onSelect={() => { markFilePickerOpen(); importInput.click(); }} />
    {#if node?.cardStyle === "card"}
      <MenuItem icon={FileArchive} label="导出为 Markdown" onSelect={() => void exportCardsMd()} />
      <MenuItem icon={Download} label="导入 Markdown 压缩包" onSelect={() => void importCardsMd()} />
    {/if}
  {/if}

  <MenuSeparator />
  <div class="menu-section-title">UI颜色</div>
  <div class="ui-color-row">
    <label class="ui-color-picker" title="修改当前界面的标题和控件颜色">
      <span style={`--swatch: ${accentValue}`}></span>
      <input type="color" value={uiColorValue} on:input={handleUiColorPick} on:change={endUiColorPick} />
    </label>
    <span class="ui-color-value">{accentValue}</span>
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
  <input bind:this={colorPickerInput} class="hidden-file" type="color" value={bg.color} on:input={handleColorPick} />
  <label class="background-link">
    背景图片链接
    <input value={linkValue} placeholder="https://..." on:focus={() => (linkLive = true)} on:input={updateBackgroundLink} on:blur={endBackgroundLinkEdit} />
  </label>
  <label class="opacity-row">
    图片透明度
    <input
      type="range"
      min="0"
      max="80"
      value={opacityValue}
      on:input={updateBackgroundOpacity}
      on:change={endBackgroundOpacityEdit}
    />
  </label>
  <div class="menu-inline two">
    <button class="menu-action-button" type="button" on:click={pickBackgroundImage}><Image size={15} /> 上传图片</button>
    <button class="menu-action-button" type="button" on:click={clearBackground}><Eraser size={15} /> 清除背景</button>
  </div>

  <input bind:this={importInput} class="hidden-file" type="file" accept="application/json,.json" on:change={importFromFile} />
  <input bind:this={diaryZipInput} class="hidden-file" type="file" accept=".zip,application/zip" on:change={importDiaryFromInput} />
  <input bind:this={cardsZipInput} class="hidden-file" type="file" accept=".zip,application/zip" on:change={importCardsMdFromInput} />
  <input bind:this={ledgerZipInput} class="hidden-file" type="file" accept=".zip,.xlsx,application/zip,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" on:change={importLedgerFromInput} />
  <input bind:this={backgroundFileInput} class="hidden-file" type="file" accept="image/*" on:change={uploadBackgroundImage} />
</ContextMenu>
