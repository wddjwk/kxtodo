<script lang="ts">
  /**
   * 分类钻取的外壳：移动端是下半屏抽屉（与其它记账浮层同一套语言），
   * 桌面端是锚在被点那一行下方的下拉面板——用户点的是排行里的一行，
   * 面板就该长在那一行旁边，而不是屏幕正中弹一个模态。
   * 内容层在 CategoryDrillBody.svelte。
   */
  import { onMount } from "svelte";
  import { isMobile, addBackInterceptor } from "../platform";
  import { anchoredPopoverStyle } from "../popover";
  import { suppressGhostClick } from "../ghostClick";
  import { appSettings } from "../stores";
  import { uiScaleValue } from "../styles";
  import { statsEntries } from "../ledger";
  import CategoryDrillBody from "./CategoryDrillBody.svelte";
  import type { LedgerBook, LedgerEntry, LedgerSide } from "../types";

  export let book: LedgerBook;
  export let entries: LedgerEntry[];
  export let categoryId: string;
  export let side: LedgerSide;
  export let from: string;
  export let to: string;
  export let periodLabel: string;
  export let anchor: HTMLElement | undefined = undefined;
  export let onClose: () => void = () => {};
  export let onEditEntry: (id: string) => void = () => {};
  export let onImageView: (id: string) => void = () => {};

  const WIDTH = 400;
  const HEIGHT = 470;

  $: category = book.categories.find((item) => item.id === categoryId);
  $: rangeEntries = statsEntries(entries, { from, to });
  $: popStyle = anchoredPopoverStyle(anchor, uiScaleValue($appSettings.appearance.uiScale), WIDTH, HEIGHT);

  function close(): void {
    onClose();
  }

  function edit(id: string): void {
    onEditEntry(id);
  }

  function handleBackdrop(event: PointerEvent): void {
    if (event.target !== event.currentTarget) return;
    // 抽屉在 pointerdown 阶段就拆掉，触屏补发的 click 会落到下面的卡片/「+」上
    event.preventDefault();
    suppressGhostClick({ x: event.clientX, y: event.clientY });
    onClose();
  }

  function handlePointerDown(event: PointerEvent): void {
    if ($isMobile) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest(".ledger-drill-pop")) return;
    if (anchor && (target === anchor || anchor.contains(target))) return;
    close();
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.isComposing || event.keyCode === 229) return;
    event.preventDefault();
    event.stopPropagation();
    close();
  }

  onMount(() => {
    window.addEventListener("pointerdown", handlePointerDown, true);
    window.addEventListener("keydown", handleKeydown, true);
    const release = addBackInterceptor(() => {
      close();
      return true;
    });
    return () => {
      window.removeEventListener("pointerdown", handlePointerDown, true);
      window.removeEventListener("keydown", handleKeydown, true);
      release();
    };
  });
</script>

{#if category}
  {#if $isMobile}
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="editor-overlay ledger-overlay" on:pointerdown={handleBackdrop} on:contextmenu|preventDefault|stopPropagation>
      <div class="editor-dialog ledger-sheet ledger-drill-sheet" role="dialog" aria-label="{category.name} 明细" tabindex="-1"
        on:pointerdown|stopPropagation on:click|stopPropagation>
        <CategoryDrillBody {book} {rangeEntries} {category} {side} {periodLabel} onClose={close} onEditEntry={edit} onImageView={onImageView} />
      </div>
    </div>
  {:else}
    <div class="ledger-drill-pop" style={popStyle} role="dialog" aria-label="{category.name} 明细" tabindex="-1"
      on:click|stopPropagation on:pointerdown|stopPropagation on:contextmenu|preventDefault|stopPropagation>
      <CategoryDrillBody {book} {rangeEntries} {category} {side} {periodLabel} onClose={close} onEditEntry={edit} onImageView={onImageView} />
    </div>
  {/if}
{/if}
