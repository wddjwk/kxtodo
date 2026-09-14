<script lang="ts">
  /**
   * 记账面板（记一笔 / 改一笔）：复用应用的 .editor-overlay 浮层——桌面居中对话框、
   * 移动端底部抽屉（主流记账 App 的同一条手感）。
   * 结构自上而下：类型页签 → 分类圆形图标网格（点大类在**它所在那一行的下面**展开
   * 一块全宽阴影的二级区——不是所有图标的下方；没有二级的大类点一下即选中）→
   * 「添加备注」+ 金额一行 → 日期/账户/图片一行（纯文字按钮，不带框）→
   * 移动端数字键盘（桌面是底部两个按钮）。
   * 移动端抽屉高度恒定且刚好放下五排分类图标：分类区自己滚动（藏滚动条），
   * 图标多了滚动、少了也不缩矮浮窗（高度飘着手感就飘）。
   *
   * **只有显式点保存才落盘**：关掉（X / 遮罩 / Esc / 移动端返回）一律丢弃草稿。
   * 卡片编辑器"关掉即保存"是为了防丢正文，账目照抄那条会凭空多出用户没确认过的记录。
   *
   * **三个按钮的语义是死的**：「记一笔 / 保存修改」= 落盘并关掉，「保存再记」= 落盘、
   * 清空金额备注与图片、面板留着连着记。编辑已有的一笔时不给「保存再记」——改一笔账
   * 之后再顺手记一笔是两件事，混在一个按钮上就是"点了保存又弹出来"的来源。
   *
   * **图片是多张**：没有图时点相机按钮选图（可多选）；已有图时按钮点亮并带数量角标，
   * 点它进全屏预览——预览右上角是 添加(+) / 删除(垃圾桶) / 关闭(X) 三个同风格圆钮，
   * 删除只改草稿，落盘仍要显式点保存。
   */
  import { onMount, tick } from "svelte";
  import {
    ArrowLeftRight, CalendarDays, Camera, Check, ChevronRight, Delete, Image as ImageIcon,
    Plus, Trash2, Wallet, X
  } from "@lucide/svelte";
  import { appSettings, showToast, todayIso, ledgerCategoryDraft, fileToDataUrl } from "../stores";
  import { isMobile } from "../platform";
  import { caps } from "../capabilities";
  import { imeInset } from "../imeInset";
  import { fieldKeydown } from "../shortcuts";
  import { clampPopoverToViewport } from "../popover";
  import { suppressGhostClick } from "../ghostClick";
  import { displayClock } from "../clock";
  import { ledgerAccent, uiScaleValue } from "../styles";
  import { categoryTree, formatCents, parseYuanToCents, LEDGER_IMAGE_NODE } from "../ledger";
  import { accountIconName, ledgerIcon, softColor, TRANSFER_ICON } from "../ledgerIcons";
  import {
    addLedgerEntry, transferLedger, updateLedgerEntry, deleteLedgerEntry
  } from "../actions";
  import { isTauriRuntime, pickImageFiles, saveMdImage, saveMdImageFromDataUrl, mdImageUrl } from "../backend";
  import { relativeDayLabel, todayDate } from "../diary";
  import DatePicker from "../DatePicker.svelte";
  import LedgerImagePreview from "./LedgerImagePreview.svelte";
  import type { LedgerBook, LedgerEditorTarget, LedgerKind, LedgerSide } from "../types";

  export let target: LedgerEditorTarget;
  export let book: LedgerBook;
  export let onClose: () => void = () => {};

  const existing = "id" in target ? book.entries.find((entry) => entry.id === target.id) : undefined;

  let kind: LedgerKind = existing?.kind ?? ("kind" in target ? target.kind : "expense");
  let amountText = existing ? (existing.amountCents / 100).toFixed(2).replace(/\.?0+$/, "") : "";
  let accountId = existing?.accountId ?? book.accounts[0]?.id ?? "";
  let toAccountId = existing?.toAccountId ?? book.accounts.find((item) => item.id !== accountId)?.id ?? "";
  let categoryId = existing?.categoryId ?? "";
  let date = existing?.date ?? ("date" in target ? target.date : todayIso());
  /** HH:MM；新建时留空——core 落盘会按当前时刻补，不必在前端猜 */
  let time = existing ? displayClock(existing.time) : "";
  let note = existing?.note ?? "";
  let images: string[] = existing?.images ? [...existing.images] : [];
  let busy = false;
  let closed = false;
  let openPicker: "" | "date" | "account" | "toAccount" = "";
  let sheetEl: HTMLDivElement;
  let imageFileInput: HTMLInputElement;
  /** 全屏图片管理（预览 + 添加 + 删除）；null = 没开 */
  let viewerItems: { src: string; title: string }[] | null = null;

  /** 展开二级区的大类；跟着已选分类走，新建时收着 */
  let openParent = book.categories.find((item) => item.id === categoryId)?.parentId ?? "";

  $: side = (kind === "income" ? "income" : "expense") as LedgerSide;
  $: tree = categoryTree(book, side);
  $: selectedCategory = book.categories.find((item) => item.id === categoryId);
  $: openChildren = tree.find((item) => item.parent.id === openParent)?.children ?? [];
  $: cents = parseYuanToCents(amountText) ?? 0;
  $: account = book.accounts.find((item) => item.id === accountId);
  $: toAccount = book.accounts.find((item) => item.id === toAccountId);
  $: today = todayDate();
  $: dateLabel = date === today ? "今天" : relativeDayLabel(date, today);
  $: accent = ledgerAccent($appSettings.ledger);
  $: sideColor = kind === "income" ? "#2f9e6e" : kind === "transfer" ? "#6b7fd7" : "#e0654f";
  $: sideDefault = side === "income" ? "#2f9e6e" : "#f0862c";

  /** 网格列数是死的（移动端 5、桌面 7，与 CSS 的 repeat() 一致）：
   *  二级阴影区要插在「大类所在行的行尾」之后，行尾位置只能靠列数算。 */
  $: cols = $isMobile ? 5 : 7;
  $: cells = [
    ...tree.map((item) => ({
      id: item.parent.id,
      name: item.parent.name,
      icon: item.parent.icon,
      color: item.parent.color || sideDefault,
      add: false
    })),
    { id: "__add", name: "新增", icon: "", color: "", add: true }
  ];
  $: openCellIndex = cells.findIndex((cell) => !cell.add && cell.id === openParent);
  $: shelfAfter =
    openCellIndex >= 0
      ? Math.min((Math.floor(openCellIndex / cols) + 1) * cols - 1, cells.length - 1)
      : -1;

  function switchKind(next: LedgerKind): void {
    if (next === kind) return;
    kind = next;
    openParent = "";
    if (next === "transfer") {
      if (toAccountId === accountId) {
        toAccountId = book.accounts.find((item) => item.id !== accountId)?.id ?? "";
      }
      return;
    }
    const nextSide: LedgerSide = next === "income" ? "income" : "expense";
    const current = book.categories.find((item) => item.id === categoryId);
    if (!current || current.side !== nextSide) categoryId = "";
  }

  function pressKey(key: string): void {
    if (key === "back") {
      amountText = amountText.slice(0, -1);
      return;
    }
    if (key === "clear") {
      amountText = "";
      return;
    }
    if (key === ".") {
      if (!amountText.includes(".")) amountText = `${amountText || "0"}.`;
      return;
    }
    const [whole, frac = ""] = amountText.split(".");
    if (frac.length >= 2) return;
    if (whole.length >= 9 && !amountText.includes(".")) return;
    amountText = amountText === "0" ? key : `${amountText}${key}`;
  }

  /** 点大类：有二级 = 在它所在行下面展开/收起阴影区；没有二级 = 直接选中记账 */
  function tapParent(id: string): void {
    const node = tree.find((item) => item.parent.id === id);
    if (!node) return;
    if (node.children.length === 0) {
      categoryId = categoryId === id ? "" : id;
      openParent = "";
      return;
    }
    openParent = openParent === id ? "" : id;
    if (openParent) {
      // 大类在滚动区下缘时，展开的阴影区可能整个在可视区外——把它带进视野
      void tick().then(() => {
        sheetEl?.querySelector(".ledger-cat-sub")?.scrollIntoView({ block: "nearest" });
      });
    }
  }

  function pickCategory(id: string): void {
    categoryId = categoryId === id ? "" : id;
  }

  /** 分类加号：分类管理长在 LedgerView 上、本面板挂在 App 层，
   *  两棵不相干的子树只能靠 store 递话（与菜单系统 menu/submenu.ts 同一套路）。 */
  function requestCategory(parentId: string): void {
    openPicker = "";
    ledgerCategoryDraft.set({ side, parentId });
  }

  function togglePicker(name: typeof openPicker): void {
    openPicker = openPicker === name ? "" : name;
    if (openPicker) {
      void clampPopoverToViewport(sheetEl, uiScaleValue($appSettings.appearance.uiScale), ".ledger-pop");
    }
  }

  function pickAccount(id: string, which: "account" | "toAccount"): void {
    if (which === "account") {
      accountId = id;
      if (kind === "transfer" && toAccountId === id) {
        toAccountId = book.accounts.find((item) => item.id !== id)?.id ?? "";
      }
    } else {
      toAccountId = id;
    }
    openPicker = "";
  }

  // ---- 多图：选择 / 预览 / 删除（全部只动草稿，落盘要显式点保存） ----

  async function resolveViewer(): Promise<void> {
    if (images.length === 0) {
      viewerItems = null;
      return;
    }
    try {
      const title = note || date;
      viewerItems = await Promise.all(
        images.map(async (name) => ({ src: await mdImageUrl(LEDGER_IMAGE_NODE, name), title }))
      );
    } catch {
      viewerItems = null;
    }
  }

  function openImageViewer(): void {
    if (images.length === 0) return;
    void resolveViewer();
  }

  async function pickImages(): Promise<void> {
    if (!isTauriRuntime) return;
    if (!caps.nativeFileDialogs) {
      // 移动端没有原生文件对话框：隐藏 file input（multiple）+ dataURL
      imageFileInput?.click();
      return;
    }
    try {
      const paths = await pickImageFiles();
      if (paths.length === 0) return;
      for (const srcPath of paths) {
        images = [...images, await saveMdImage(srcPath, LEDGER_IMAGE_NODE)];
      }
      if (viewerItems) await resolveViewer();
    } catch (error) {
      showToast(`图片添加失败：${String(error)}`);
    }
  }

  async function pickImagesFromInput(event: Event): Promise<void> {
    const input = event.currentTarget;
    if (!(input instanceof HTMLInputElement) || !input.files?.length) return;
    if (!isTauriRuntime) return;
    try {
      for (const file of [...input.files]) {
        const dataUrl = await fileToDataUrl(file);
        images = [...images, await saveMdImageFromDataUrl(dataUrl, LEDGER_IMAGE_NODE)];
      }
      if (viewerItems) await resolveViewer();
    } catch (error) {
      showToast(`图片添加失败：${String(error)}`);
    } finally {
      input.value = "";
    }
  }

  /** 预览里的垃圾桶：删掉当前这张（草稿级；删空了预览自己关掉） */
  function deleteViewerImage(index: number): void {
    images = images.filter((_, position) => position !== index);
    if (viewerItems) viewerItems = viewerItems.filter((_, position) => position !== index);
    if (images.length === 0) viewerItems = null;
  }

  function dismissPopovers(event: Event): void {
    if (!openPicker) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest(".ledger-meta-field, .ledger-transfer-field")) return;
    openPicker = "";
  }

  // 捕获阶段：对话框对 pointerdown 做了 stopPropagation，冒泡阶段收不到里面的点击
  onMount(() => {
    window.addEventListener("pointerdown", dismissPopovers, true);
    return () => window.removeEventListener("pointerdown", dismissPopovers, true);
  });

  function valid(): boolean {
    if (cents <= 0) return false;
    if (kind === "transfer") return Boolean(accountId) && Boolean(toAccountId) && accountId !== toAccountId;
    return Boolean(accountId);
  }

  /** 真正落盘。keepOpen = 「保存再记」：清掉金额、备注与图片，分类/账户/日期时刻留着连记。 */
  async function commit(keepOpen: boolean): Promise<void> {
    if (busy) return;
    busy = true;
    const isTransfer = kind === "transfer";
    // time 留空就不传：core 新建时按当前时刻补，改的时候沿用原值
    const clock = time || undefined;
    let ok = false;
    if (existing) {
      // 改一笔就是改这一笔——转账也走 modify（core 的 ledger.modify 认 kind/转入账户，
      // 空串即清除）。早先这里按 kind 分派去 ledger.transfer，于是「改转账」变成了
      // 「又记一笔新的转账」，旧的那还原封不动躺着。
      ok = await updateLedgerEntry(existing.id, {
        kind,
        amountCents: cents,
        accountId,
        toAccountId: isTransfer ? toAccountId : "",
        categoryId: isTransfer ? "" : categoryId,
        date,
        time: clock,
        note,
        images
      });
    } else if (isTransfer) {
      ok = await transferLedger({
        from: accountId,
        to: toAccountId,
        amountCents: cents,
        date,
        time: clock,
        note,
        images: images.length > 0 ? images : undefined
      });
    } else {
      ok = await addLedgerEntry({
        kind,
        amountCents: cents,
        accountId,
        categoryId: categoryId || undefined,
        date,
        time: clock,
        note,
        images: images.length > 0 ? images : undefined
      });
    }
    busy = false;
    if (!ok) return;
    if (keepOpen) {
      amountText = "";
      note = "";
      images = [];
      viewerItems = null;
      return;
    }
    closed = true;
    onClose();
  }

  /** 显式点保存：不合法要给一句话，不能默默不动。
   *  at = 这一下的指针坐标（移动端数字键盘走 pointerdown）：面板在 click 补发之前就没了，
   *  得把那一下 click 吃掉；桌面按钮走的是 click，事件已经派发给按钮本身，不用吃。 */
  async function save(keepOpen: boolean, at?: { x: number; y: number }): Promise<void> {
    if (cents <= 0) {
      showToast("金额要大于 0");
      return;
    }
    if (kind === "transfer" && accountId === toAccountId) {
      showToast("转出与转入不能是同一个账户");
      return;
    }
    if (!valid()) {
      showToast("请先选择账户");
      return;
    }
    if (!keepOpen && at) suppressGhostClick(at);
    await commit(keepOpen);
  }

  /** 关闭（X / 点遮罩 / Esc / 移动端返回）：一律**不落盘**。
   *  记账与卡片编辑器刻意不同——卡片"关掉即保存"是防丢正文，而金额这里
   *  自动落盘会凭空多出一堆用户没确认过的账，所以只有显式点「记一笔 / 保存」才写。
   *  at 同上：只有点遮罩这一下需要吃掉补发的 click，Esc 与返回键不需要。 */
  function closeEditor(at?: { x: number; y: number }): void {
    if (closed) return;
    if (at) suppressGhostClick(at);
    closed = true;
    onClose();
  }

  async function remove(): Promise<void> {
    if (!existing || busy) return;
    busy = true;
    const ok = await deleteLedgerEntry(existing.id);
    busy = false;
    if (!ok) return;
    closed = true;
    onClose();
  }

  function handleBackdrop(event: PointerEvent): void {
    if (event.target !== event.currentTarget) return;
    // 触屏上这一下的 click 会在面板消失之后才补发，不拦就会落到下面的卡片/「+」上
    event.preventDefault();
    closeEditor({ x: event.clientX, y: event.clientY });
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented) return;
    // 图片预览开着：Esc 与左右键都归它（它的监听也挂在捕获阶段，这里让位即可）
    if (viewerItems) return;
    // 分类管理/钻取面板是从这个面板里唤出的，DOM 上挂在 .ledger-view 里、z-index 更高。
    // 两层的 keydown 都挂在 window 上，本面板注册得更早所以先跑——不让位的话一下 Esc
    // 会把底下的记账面板也关掉，上面的管理器却留着（界面看起来"卡住"）。
    if (document.querySelector(".ledger-view .editor-overlay, .ledger-drill-pop")) return;
    if (event.key === "Escape" && !event.isComposing && event.keyCode !== 229) {
      event.preventDefault();
      event.stopPropagation();
      if (openPicker) {
        openPicker = "";
        return;
      }
      closeEditor();
      return;
    }
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void save(false);
      return;
    }
    // 桌面没有键盘：数字键直接喂金额（输入框聚焦时不抢）
    if ($isMobile || busy) return;
    const element = event.target as HTMLElement | null;
    if (element?.closest("input, textarea")) return;
    if (/^[0-9.]$/.test(event.key)) {
      event.preventDefault();
      pressKey(event.key);
    } else if (event.key === "Backspace") {
      event.preventDefault();
      pressKey("back");
    } else if (event.key === "Enter") {
      event.preventDefault();
      void save(false);
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="editor-overlay ledger-overlay"
  style={`--side: ${sideColor}`}
  use:imeInset
  on:pointerdown={handleBackdrop}
  on:contextmenu|preventDefault|stopPropagation
>
  <div
    class="editor-dialog ledger-sheet ledger-editor-sheet"
    class:ledger-transfer-sheet={kind === "transfer"}
    bind:this={sheetEl}
    style={`--accent: ${accent}`}
    role="dialog"
    aria-label={existing ? "修改这一笔" : "记一笔"}
    tabindex="-1"
    on:pointerdown|stopPropagation
    on:click|stopPropagation
  >
    <header class="ledger-sheet-head">
      <div class="ledger-kind-tabs" role="tablist">
        {#each [["expense", "支出"], ["income", "收入"], ["transfer", "转账"]] as [value, label] (value)}
          <button
            type="button"
            role="tab"
            aria-selected={kind === value}
            class:active={kind === value}
            on:click={() => switchKind(value as LedgerKind)}
          >{label}</button>
        {/each}
      </div>
      <div class="ledger-sheet-actions">
        {#if $isMobile && !existing}
          <button type="button" class="ledger-head-text" title="保存并接着记下一笔" on:click={() => void save(true)}>
            保存再记
          </button>
        {/if}
        {#if existing}
          <button type="button" class="ledger-icon-button danger" title="删除这一笔" on:click={() => void remove()}>
            <Trash2 size={17} />
          </button>
        {/if}
        <button type="button" class="ledger-icon-button" title="关闭（不保存这一笔）" aria-label="关闭" on:click={() => closeEditor()}>
          <X size={18} />
        </button>
      </div>
    </header>

    {#if kind === "transfer"}
      <!-- 转账块刻意不放进滚动区：滚动容器的 overflow 会把账户浮层裁掉，
           看起来就是「浮层被上面的金额行盖住」。 -->
      <div class="ledger-transfer-block">
        <div class="ledger-transfer-row" on:click|stopPropagation>
          <div class="ledger-transfer-field" class:open={openPicker === "account"}>
            <button type="button" on:click={() => togglePicker("account")}>
              <span>转出</span>
              <strong>{account?.name ?? "选择账户"}</strong>
              <ChevronRight size={15} />
            </button>
            {#if openPicker === "account"}
              <div class="ledger-pop">
                {#each book.accounts as item (item.id)}
                  <button type="button" class="ledger-pick-row" class:active={item.id === accountId} on:click={() => pickAccount(item.id, "account")}>
                    <svelte:component this={ledgerIcon(accountIconName(item.icon, item.kind), "Wallet")} size={15} />
                    <span>{item.name}</span>
                    {#if item.id === accountId}<Check size={14} />{/if}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
          <ArrowLeftRight class="ledger-transfer-arrow" size={17} />
          <div class="ledger-transfer-field" class:open={openPicker === "toAccount"}>
            <button type="button" on:click={() => togglePicker("toAccount")}>
              <span>转入</span>
              <strong>{toAccount?.name ?? "选择账户"}</strong>
              <ChevronRight size={15} />
            </button>
            {#if openPicker === "toAccount"}
              <div class="ledger-pop">
                {#each book.accounts as item (item.id)}
                  <button type="button" class="ledger-pick-row" class:active={item.id === toAccountId} on:click={() => pickAccount(item.id, "toAccount")}>
                    <svelte:component this={ledgerIcon(accountIconName(item.icon, item.kind), "Wallet")} size={15} />
                    <span>{item.name}</span>
                    {#if item.id === toAccountId}<Check size={14} />{/if}
                  </button>
                {/each}
              </div>
            {/if}
          </div>
        </div>
        <p class="ledger-sheet-hint">转账只改两个账户的余额，不计入收支统计。</p>
      </div>
    {:else}
      <div class="ledger-cat-zone">
        {#if tree.length === 0}
          <p class="ledger-sheet-hint">还没有{side === "income" ? "收入" : "支出"}分类，点加号加一个。</p>
        {/if}
        <div class="ledger-cat-grid">
          {#each cells as cell, i (cell.id)}
            {#if cell.add}
              <button type="button" class="ledger-cat-cell add" title="新增大类" on:click={() => requestCategory("")}>
                <span class="ledger-cat-round add"><Plus size={20} /></span>
                <em>新增</em>
              </button>
            {:else}
              <button
                type="button"
                class="ledger-cat-cell"
                class:open={openParent === cell.id}
                class:active={categoryId === cell.id}
                style="--cat: {cell.color}"
                on:click={() => tapParent(cell.id)}
              >
                <span class="ledger-cat-round" style="background: {cell.color}">
                  <svelte:component this={ledgerIcon(cell.icon, "Package")} size={20} />
                </span>
                <em>{cell.name}</em>
              </button>
            {/if}
            <!-- 二级阴影区插在「大类所在行的行尾」之后：grid 的整宽子项自动换行，
                 于是它正好出现在那一行的下面一行，而不是所有图标的末尾 -->
            {#if i === shelfAfter && openChildren.length > 0}
              <div class="ledger-cat-sub">
                <div class="ledger-cat-grid">
                  {#each openChildren as tile (tile.id)}
                    {@const color = tile.color || sideDefault}
                    <button
                      type="button"
                      class="ledger-cat-cell small"
                      class:active={categoryId === tile.id}
                      style="--cat: {color}"
                      on:click={() => pickCategory(tile.id)}
                    >
                      <span class="ledger-cat-round" style="background: {color}">
                        <svelte:component this={ledgerIcon(tile.icon, "Package")} size={17} />
                      </span>
                      <em>{tile.name}</em>
                    </button>
                  {/each}
                  <button
                    type="button"
                    class="ledger-cat-cell small add"
                    title="在「{tree.find((item) => item.parent.id === openParent)?.parent.name ?? ""}」下新增子分类"
                    on:click={() => requestCategory(openParent)}
                  >
                    <span class="ledger-cat-round add"><Plus size={17} /></span>
                    <em>新增</em>
                  </button>
                </div>
              </div>
            {/if}
          {/each}
        </div>
      </div>
    {/if}

    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="ledger-note-row" on:click|stopPropagation>
      <label class="ledger-note-plain" title="备注">
        <input type="text" maxlength="120" placeholder="添加备注" bind:value={note} on:keydown={fieldKeydown} />
      </label>
      {#if $isMobile}
        <!-- 移动端金额只由键盘驱动：再放一个可聚焦的输入框会和软键盘抢位 -->
        <div class="ledger-amount-value" class:empty={!amountText}>
          {amountText || "0.00"}<i class="ledger-amount-caret"></i>
        </div>
      {:else}
        <label class="ledger-amount-plain">
          <input
            type="text"
            inputmode="decimal"
            autocomplete="off"
            placeholder="0.00"
            aria-label="金额（元）"
            style="width: {Math.max(4, amountText.length + 1)}ch"
            bind:value={amountText}
          />
        </label>
      {/if}
    </div>

    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div class="ledger-meta-row" on:click|stopPropagation>
      <div class="ledger-meta-field" class:open={openPicker === "date"}>
        <button type="button" class="ledger-meta-plain" title="归属日期与时刻" on:click={() => togglePicker("date")}>
          <CalendarDays size={15} />{dateLabel}{#if time}<em class="ledger-meta-clock">{time}</em>{/if}
        </button>
        {#if openPicker === "date"}
          <div class="ledger-pop date">
            <DatePicker
              value={date}
              {time}
              withTime
              on:select={(event) => { date = event.detail; openPicker = ""; }}
              on:selectTime={(event) => { time = event.detail; }}
              on:clear={() => { date = today; openPicker = ""; }}
            />
          </div>
        {/if}
      </div>

      {#if kind !== "transfer"}
        <div class="ledger-meta-field" class:open={openPicker === "account"}>
          <button type="button" class="ledger-meta-plain" title="资金账户" on:click={() => togglePicker("account")}>
            <Wallet size={15} />{account?.name ?? "选择账户"}
          </button>
          {#if openPicker === "account"}
            <div class="ledger-pop">
              {#each book.accounts as item (item.id)}
                <button type="button" class="ledger-pick-row" class:active={item.id === accountId} on:click={() => pickAccount(item.id, "account")}>
                  <svelte:component this={ledgerIcon(accountIconName(item.icon, item.kind), "Wallet")} size={15} />
                  <span>{item.name}</span>
                  {#if item.id === accountId}<Check size={14} />{/if}
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      <span class="ledger-meta-spacer"></span>
      {#if images.length > 0}
        <button
          type="button"
          class="ledger-meta-plain has-image"
          title="查看与管理这条账的图片"
          on:click={openImageViewer}
        >
          <ImageIcon size={16} />
          {#if images.length > 1}<i class="ledger-image-badge">{images.length}</i>{/if}
        </button>
      {:else}
        <button type="button" class="ledger-meta-plain" title="给这条账添加图片（可多选）" on:click={() => void pickImages()}>
          <Camera size={16} />
        </button>
      {/if}
      <input type="file" hidden accept="image/*" multiple bind:this={imageFileInput} on:change={pickImagesFromInput} />
    </div>

    {#if $isMobile}
      <div class="ledger-keypad">
        {#each ["7", "8", "9"] as key (key)}
          <button type="button" class="ledger-key" on:pointerdown|preventDefault={() => pressKey(key)}>{key}</button>
        {/each}
        <button type="button" class="ledger-key fn" title="退格" on:pointerdown|preventDefault={() => pressKey("back")}>
          <Delete size={19} />
        </button>
        {#each ["4", "5", "6"] as key (key)}
          <button type="button" class="ledger-key" on:pointerdown|preventDefault={() => pressKey(key)}>{key}</button>
        {/each}
        <button type="button" class="ledger-key fn" on:pointerdown|preventDefault={() => pressKey("clear")}>清除</button>
        {#each ["1", "2", "3"] as key (key)}
          <button type="button" class="ledger-key" on:pointerdown|preventDefault={() => pressKey(key)}>{key}</button>
        {/each}
        <button
          type="button"
          class="ledger-key save"
          disabled={busy}
          on:pointerdown|preventDefault={(event) => void save(false, { x: event.clientX, y: event.clientY })}
        >
          <Check size={18} />{existing ? "保存" : "记一笔"}
        </button>
        <button type="button" class="ledger-key zero" on:pointerdown|preventDefault={() => pressKey("0")}>0</button>
        <button type="button" class="ledger-key" on:pointerdown|preventDefault={() => pressKey(".")}>.</button>
      </div>
    {:else}
      <footer class="ledger-sheet-foot">
        <span class="ledger-foot-hint">
          {#if cents > 0}{formatCents(cents)} 元{/if}
        </span>
        <span class="ledger-foot-spacer"></span>
        {#if !existing}
          <button type="button" class="settings-button" disabled={busy} on:click={() => void save(true)}>保存再记</button>
        {/if}
        <button type="button" class="settings-button primary" disabled={busy} on:click={() => void save(false)}>
          {existing ? "保存修改" : "记一笔"}
        </button>
      </footer>
    {/if}
  </div>
</div>

{#if viewerItems}
  <LedgerImagePreview
    items={viewerItems}
    editable
    onAdd={() => void pickImages()}
    onDelete={deleteViewerImage}
    onClose={() => (viewerItems = null)}
  />
{/if}
