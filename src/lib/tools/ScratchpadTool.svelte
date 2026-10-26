<script lang="ts">
  /**
   * 草稿纸（v0.8.4 需求 2；v0.9.0 标签页化）：多页纯文本便签。不渲染、不解析、
   * 不做任何 markdown 处理——「跟记事本一样」就是它的全部规格。
   *
   * **标签页**：只有一个时不展示标签栏（与旧版一模一样）；新建走子页头部的加号
   * （ToolboxView 经 `scratchpadNewTab` store 递话）；点击切换、再点已激活的开始
   * 改名；悬浮右侧露出 X 关闭；右键菜单 = 关闭当前 / 关闭其他 / 关闭右侧。
   * 宽度按容器与页数自动伸缩（min/max 钳制、左对齐、放不下横向滚动）。
   *
   * **默认名从内容派生（v0.9.1 需求 18）**：机器命名的标签页（`nameAuto: true`）
   * 名字随正文重派生——取第一行有内容的文本、剥 markdown 噪音、按标签页最小宽度
   * 的宽度预算截断（`scratchpadName.ts::autoTabName`，纯函数有单测）；没有内容可取
   * 时回落纯数字名（最大值 + 1 且跳过已占用）。用户手动改过名（`nameAuto: false`）
   * 就永不再动。
   *
   * **存储与同步**：全部标签页住在数据域（`data.json` 的 `scratchpad.tabs`），
   * 跟着「同步数据」范围一起走（LWW 整份覆盖）。localStorage 只留作**首帧兜底**。
   *
   * **保存时机**：文本输入防抖 5 秒 + 切后台/关页面前 flush；标签页结构变更
   * （新建/改名/关闭/切换）立即写——结构操作低频，等 5 秒反而会在连点时丢步骤。
   *
   * **光标残留**（v0.9.0 顺带整治）：WebView 在缩放壳里点选 textarea 偶发把旧
   * 光标栅格化残影留在原位。点击后用一次「scrollTop ±1」的不可见重绘把残影刷掉。
   */
  import { get } from "svelte/store";
  import { onDestroy, onMount, tick } from "svelte";
  import { X } from "@lucide/svelte";
  import { appState, scratchpadNewTab } from "../stores";
  import { setScratchpadTabs } from "../actions";
  import { autoTabName } from "../scratchpadName";
  import ContextMenu from "../menu/ContextMenu.svelte";
  import MenuItem from "../menu/MenuItem.svelte";
  import { createBackGuard } from "../platform";
  import type { ScratchpadTab } from "../types";

  const STORAGE_KEY = "kxtodo-scratchpad-v2";
  const SAVE_DEBOUNCE_MS = 5000;

  type LocalTab = { id: string; name: string; nameAuto: boolean; text: string };

  let tabs: LocalTab[] = [];
  let activeId = "";
  let text = "";
  let timer: number | undefined;
  let seeded = false;
  /** 有未 flush 的输入（水合到来的覆盖要避让，见下面的 seed 分支） */
  let dirty = false;
  let lastWritten = "";

  let renamingId = "";
  let renameDraft = "";
  let renameInput: HTMLInputElement;
  let tabMenu: { id: string; x: number; y: number } | null = null;
  let areaEl: HTMLTextAreaElement;

  const backGuard = createBackGuard();
  $: backGuard(tabMenu !== null, () => (tabMenu = null));

  function newTabId(): string {
    return `tab-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
  }

  /**
   * 无内容可取时的数字回落名（v0.9.0 的默认名逻辑）：现有纯数字名的最大值 + 1
   * （"1"、"2"、"3"…），并跳过已占用的名字——用户把 "2" 改名成「会议」后再新建，
   * 纯按最大值算会撞回 "1"。`excludeId` = 给某个已有标签页重算回落名时把它自己
   * 排除掉（否则它自己的数字名会把自己顶到下一个号）。
   */
  function nextTabName(excludeId = ""): string {
    let max = 0;
    const used = new Set<string>();
    for (const tab of tabs) {
      if (tab.id === excludeId) continue;
      const parsed = Number(tab.name);
      if (Number.isInteger(parsed) && parsed > max) max = parsed;
      used.add(tab.name);
    }
    let candidate = max + 1;
    while (used.has(String(candidate))) candidate += 1;
    return String(candidate);
  }

  function defaultTabs(): LocalTab[] {
    return [{ id: "tab-1", name: "1", nameAuto: true, text: "" }];
  }

  function indexOf(id: string): number {
    return tabs.findIndex((tab) => tab.id === id);
  }

  /** 把工作区文本合回 tabs（活动页的 text 以 `text` 变量为准） */
  function mergedTabs(): LocalTab[] {
    return tabs.map((tab) => (tab.id === activeId ? { ...tab, text } : tab));
  }

  function writeCache(next: LocalTab[], nextActiveId: string): void {
    try {
      const json = JSON.stringify({ tabs: next, activeTabId: nextActiveId });
      // 值没变一个字节都不写：同步 setItem 是同步磁盘 I/O
      if (localStorage.getItem(STORAGE_KEY) !== json) localStorage.setItem(STORAGE_KEY, json);
    } catch {
      // 配额满：缓存失败无所谓，正文已经进领域文件
    }
  }

  /** 落盘。`immediate` = 结构变更（不等防抖）。内容没变一个字节都不写。 */
  function flush(immediate = false): void {
    void immediate;
    window.clearTimeout(timer);
    timer = undefined;
    const next = mergedTabs();
    const payload: ScratchpadTab[] = next.map((tab) => ({
      id: tab.id,
      name: tab.name,
      nameAuto: tab.nameAuto,
      text: tab.text,
      updatedAt: new Date().toISOString()
    }));
    const signature = JSON.stringify({ tabs: next, activeTabId: activeId });
    const store = get(appState).scratchpad;
    const storeSignature = JSON.stringify({
      tabs: store.tabs.map((tab) => ({ id: tab.id, name: tab.name, nameAuto: tab.nameAuto, text: tab.text })),
      activeTabId: store.activeTabId
    });
    dirty = false;
    writeCache(next, activeId);
    // 与 store 一致（刚 seed 完 / 自己那笔写回来了）或与自己上次写的相同 → 一个字节都不写，
    // 否则每次打开草稿纸都会白抬一次 data 域 revision、灌一条审计
    if (signature === storeSignature || signature === lastWritten) {
      lastWritten = signature;
      return;
    }
    lastWritten = signature;
    void setScratchpadTabs(payload, activeId);
  }

  function handleInput(): void {
    dirty = true;
    // 活动页文本立即合进 tabs（切换/关闭标签不丢字），落盘照旧防抖。
    // 机器命名的活动页顺带随内容重派生默认名（v0.9.1 需求 18）；手动改过名的不动。
    tabs = mergedTabs().map((tab) => {
      if (tab.id !== activeId || !tab.nameAuto) return tab;
      const derived = autoTabName(tab.text);
      const name = derived !== "" ? derived : nextTabName(tab.id);
      return name === tab.name ? tab : { ...tab, name };
    });
    window.clearTimeout(timer);
    timer = window.setTimeout(() => flush(true), SAVE_DEBOUNCE_MS);
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Tab" || event.shiftKey || event.ctrlKey || event.altKey || event.metaKey || event.isComposing) return;
    event.preventDefault();
    document.execCommand("insertText", false, "\t");
  }

  /**
   * 光标残留的重绘兜底：缩放壳 + GPU 合成下，WebView 偶发把旧光标的栅格化残影留在
   * 原位（点 a 再点 b，a 处还有一条影子）。两招都不可见：① 滚动 1px 再滚回来（内容
   * 够长时触发一次重绘）；② 一帧 opacity 0.999（内容不足一屏时滚动挪不动，靠它强制
   * 元素重绘）。用 opacity 不用 transform——translateZ 会临时提升合成层，在缩放壳里
   * 可能闪一下重采样。
   */
  function nudgeCaret(): void {
    const el = areaEl;
    if (!el) return;
    const top = el.scrollTop;
    el.scrollTop = top + 1;
    el.scrollTop = top;
    el.style.opacity = "0.999";
    requestAnimationFrame(() => {
      el.style.opacity = "";
    });
  }

  function onVisibility(): void {
    if (document.visibilityState === "hidden" && dirty) flush(true);
  }

  function onLeave(): void {
    if (dirty) flush(true);
  }

  onMount(() => {
    // 首帧：先画缓存里的字（水合还没来）。已经 seed 过就别拿缓存盖回去
    // ——store 已经是权威，缓存可能更旧。
    if (!seeded) {
      try {
        const cached = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null") as {
          tabs?: LocalTab[];
          activeTabId?: string;
        } | null;
        if (cached && Array.isArray(cached.tabs) && cached.tabs.length > 0) {
          tabs = cached.tabs
            .filter((tab) => tab && typeof tab.id === "string" && tab.id !== "")
            // 归一成 LocalTab 的键序与类型（老缓存可能缺 nameAuto；缺 = 手动名，宁可不覆盖）
            .map((tab) => ({
              id: tab.id,
              name: typeof tab.name === "string" ? tab.name : "",
              nameAuto: tab.nameAuto === true,
              text: typeof tab.text === "string" ? tab.text : ""
            }));
          activeId = tabs.some((tab) => tab.id === cached.activeTabId) ? (cached.activeTabId as string) : tabs[0].id;
          text = tabs.find((tab) => tab.id === activeId)?.text ?? "";
        }
      } catch {
        // 缓存坏了就当没有
      }
      if (tabs.length === 0) {
        tabs = defaultTabs();
        activeId = tabs[0].id;
        text = "";
      }
      lastWritten = JSON.stringify({ tabs, activeTabId: activeId });
    }
    window.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("pagehide", onLeave);
  });

  // 数据域（含同步拉回来的）是权威：水合/同步一到就覆盖本地缓存那一份，
  // 但**只覆盖一次**——之后用户在页面上的输入不能被它拉回去。
  $: if (!seeded && $appState.scratchpad.updatedAt) {
    seeded = true;
    // 水合到达时用户可能已经打了字、防抖还没 flush：脏输入优先。
    // 无提示地拿旧值盖掉刚打的字，是比「晚几秒合并」严重得多的伤害。
    if (!dirty) {
      const store = $appState.scratchpad;
      if (store.tabs.length > 0) {
        tabs = store.tabs.map((tab) => ({ id: tab.id, name: tab.name, nameAuto: tab.nameAuto === true, text: tab.text }));
        activeId = tabs.some((tab) => tab.id === store.activeTabId) ? store.activeTabId : tabs[0].id;
        text = tabs.find((tab) => tab.id === activeId)?.text ?? "";
        lastWritten = JSON.stringify({ tabs, activeTabId: activeId });
      }
    }
  }

  // ---- 标签页操作（结构变更一律立即落盘） ----

  function addTab(): void {
    if (dirty) flush(true);
    // 新页没有内容 → 数字回落名；开始打字后由 handleInput 换成内容派生名
    const tab: LocalTab = { id: newTabId(), name: nextTabName(), nameAuto: true, text: "" };
    tabs = [...mergedTabs(), tab];
    activeId = tab.id;
    text = "";
    flush(true);
    void tick().then(() => areaEl?.focus());
  }

  // 子页头部的加号（ToolboxView）经 store 递话过来
  let seenNewTab = get(scratchpadNewTab);
  $: if ($scratchpadNewTab > seenNewTab) {
    seenNewTab = $scratchpadNewTab;
    addTab();
  }

  function handleTabClick(tab: LocalTab): void {
    tabMenu = null;
    if (tab.id === activeId) {
      // 再点已激活的 = 改名
      startRename(tab);
      return;
    }
    if (dirty) flush(true);
    tabs = mergedTabs();
    activeId = tab.id;
    text = tabs.find((item) => item.id === tab.id)?.text ?? "";
    dirty = false;
    flush(true);
    void tick().then(() => areaEl?.focus());
  }

  function startRename(tab: LocalTab): void {
    renamingId = tab.id;
    renameDraft = tab.name;
    void tick().then(() => {
      renameInput?.focus();
      renameInput?.select();
    });
  }

  function commitRename(): void {
    if (!renamingId) return;
    const id = renamingId;
    const name = renameDraft.trim().slice(0, 40);
    renamingId = "";
    // 用户改了名 → nameAuto 落 false，之后正文怎么变都不再覆盖这个名字。
    // 空名（清空后失焦）= 放弃这次改名，名字与标记都保持原样。
    tabs = tabs.map((tab) => (tab.id === id ? { ...tab, name: name || tab.name, nameAuto: name ? false : tab.nameAuto } : tab));
    flush(true);
  }

  function handleRenameKeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Enter") {
      event.preventDefault();
      commitRename();
    } else if (event.key === "Escape") {
      event.preventDefault();
      renamingId = "";
    }
  }

  /** 关闭标签页；剩最后一个时不允许关（单页 = 与旧版草稿纸同一形态） */
  function closeTab(id: string): void {
    tabMenu = null;
    if (tabs.length <= 1) return;
    if (dirty) flush(true);
    const current = mergedTabs();
    const at = current.findIndex((tab) => tab.id === id);
    if (at < 0) return;
    const next = current.filter((tab) => tab.id !== id);
    tabs = next;
    if (activeId === id) {
      const neighbor = next[Math.min(at, next.length - 1)];
      activeId = neighbor.id;
      text = neighbor.text;
    }
    dirty = false;
    flush(true);
  }

  function closeOthers(id: string): void {
    tabMenu = null;
    if (dirty) flush(true);
    const current = mergedTabs();
    const keep = current.filter((tab) => tab.id === id);
    if (keep.length === 0) return;
    tabs = keep;
    activeId = id;
    text = keep[0].text;
    dirty = false;
    flush(true);
  }

  function closeRightOf(id: string): void {
    tabMenu = null;
    const at = indexOf(id);
    if (at < 0 || at === tabs.length - 1) return;
    if (dirty) flush(true);
    const current = mergedTabs();
    tabs = current.slice(0, at + 1);
    if (!tabs.some((tab) => tab.id === activeId)) {
      activeId = id;
      text = tabs[at].text;
    }
    dirty = false;
    flush(true);
  }

  function handleTabContext(event: MouseEvent, id: string): void {
    event.preventDefault();
    event.stopPropagation();
    tabMenu = { id, x: event.clientX, y: event.clientY };
  }

  onDestroy(() => {
    window.removeEventListener("visibilitychange", onVisibility);
    window.removeEventListener("pagehide", onLeave);
    if (dirty) flush(true);
  });
</script>

<div class="scratchpad">
  {#if tabs.length > 1}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="scratchpad-tabs" on:contextmenu|preventDefault>
      {#each tabs as tab (tab.id)}
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
        <div
          class="scratchpad-tab"
          class:active={tab.id === activeId}
          role="tab"
          aria-selected={tab.id === activeId}
          title={tab.name}
          on:click={() => handleTabClick(tab)}
          on:contextmenu={(event) => handleTabContext(event, tab.id)}
        >
          {#if renamingId === tab.id}
            <input
              bind:this={renameInput}
              bind:value={renameDraft}
              class="scratchpad-tab-rename"
              maxlength="40"
              on:blur={commitRename}
              on:keydown={handleRenameKeydown}
              on:click|stopPropagation
            />
          {:else}
            <span class="scratchpad-tab-name">{tab.name}</span>
          {/if}
          <button
            class="scratchpad-tab-close"
            type="button"
            aria-label="关闭标签页 {tab.name}"
            title="关闭标签页"
            on:click|stopPropagation={() => closeTab(tab.id)}
          ><X size={12} strokeWidth={3} /></button>
        </div>
      {/each}
    </div>
  {/if}

  <textarea
    bind:this={areaEl}
    class="scratchpad-area"
    bind:value={text}
    on:input={handleInput}
    on:keydown={handleKeydown}
    on:mouseup={nudgeCaret}
    on:touchend={nudgeCaret}
    placeholder="随手记点什么…（纯文本，不渲染，自动保存）"
    spellcheck="false"
  ></textarea>
  <!-- 字数统计：需求 2 只要求隐藏「正在输入/已自动保存」，计数留着（v0.8.5 需求 6） -->
  <div class="scratchpad-foot"><span class="scratchpad-count">{text.length} 字</span></div>

  {#if tabMenu}
    <ContextMenu x={tabMenu.x} y={tabMenu.y} minWidth={180} onClose={() => (tabMenu = null)}>
      <MenuItem label="关闭当前标签页" onSelect={() => closeTab(tabMenu?.id ?? "")} />
      <MenuItem label="关闭其他标签页" onSelect={() => closeOthers(tabMenu?.id ?? "")} />
      <MenuItem label="关闭右侧标签页" onSelect={() => closeRightOf(tabMenu?.id ?? "")} />
    </ContextMenu>
  {/if}
</div>
