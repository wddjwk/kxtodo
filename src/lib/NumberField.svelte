<script lang="ts">
  /**
   * 严格的数字输入：原生 number 框（内置加减，step=1 保证严格 ±1）；
   * 显示永远等于用户输入（聚焦期间外部值不回灌），失焦/回车才 clamp 写回。
   */
  export let value = 0;
  export let min = 0;
  export let max = 100;
  export let suffix = "";
  export let ariaLabel = "";
  /** 输入过程中数值合法（在 min/max 内）就即时提交，用于缩放/字号的实时预览。 */
  export let live = false;
  export let onCommit: (value: number) => void = () => {};

  let draft = String(value);
  let focused = false;

  $: if (!focused) draft = String(value);

  function parse(text: string): number | null {
    const trimmed = text.trim();
    if (trimmed === "") return null;
    const parsed = Number(trimmed);
    return Number.isFinite(parsed) ? parsed : null;
  }

  function handleInput(event: Event): void {
    draft = (event.currentTarget as HTMLInputElement).value;
    if (!live) return;
    const parsed = parse(draft);
    if (parsed !== null && parsed >= min && parsed <= max) onCommit(Math.round(parsed));
  }

  function commit(): void {
    const parsed = parse(draft);
    if (parsed === null) {
      draft = String(value);
      return;
    }
    const next = Math.min(max, Math.max(min, Math.round(parsed)));
    draft = String(next);
    onCommit(next);
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Enter") {
      event.preventDefault();
      commit();
    }
  }
</script>

<span class="number-field">
  <input
    type="number"
    step="1"
    {min}
    {max}
    aria-label={ariaLabel || undefined}
    value={draft}
    on:input={handleInput}
    on:focus={() => (focused = true)}
    on:blur={() => { focused = false; commit(); }}
    on:keydown={handleKeydown}
  />
  {#if suffix}<span class="number-suffix">{suffix}</span>{/if}
</span>
