<script lang="ts">
  /**
   * 严格的数字输入：显示永远等于用户输入（聚焦期间外部值不回灌），
   * 步进按钮与方向键严格 ±1，提交（失焦/回车/步进）时才 clamp 并写回。
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
    if (!/^\d+$/.test(trimmed)) return null;
    const parsed = Number(trimmed);
    return Number.isFinite(parsed) ? parsed : null;
  }

  function apply(next: number): void {
    draft = String(next);
    onCommit(next);
  }

  function handleInput(): void {
    if (!live) return;
    const parsed = parse(draft);
    if (parsed !== null && parsed >= min && parsed <= max) onCommit(parsed);
  }

  function commit(): void {
    const parsed = parse(draft);
    if (parsed === null) {
      draft = String(value);
      return;
    }
    apply(Math.min(max, Math.max(min, parsed)));
  }

  function step(delta: number): void {
    const base = parse(draft) ?? value;
    apply(Math.min(max, Math.max(min, base + delta)));
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Enter") {
      event.preventDefault();
      commit();
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      step(1);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      step(-1);
    }
  }
</script>

<span class="number-field">
  <button
    type="button"
    class="number-step"
    tabindex="-1"
    aria-label="减少"
    on:mousedown|preventDefault
    on:click={() => step(-1)}
  >−</button>
  <input
    type="text"
    inputmode="numeric"
    aria-label={ariaLabel || undefined}
    bind:value={draft}
    on:input={handleInput}
    on:focus={() => (focused = true)}
    on:blur={() => { focused = false; commit(); }}
    on:keydown={handleKeydown}
  />
  <button
    type="button"
    class="number-step"
    tabindex="-1"
    aria-label="增加"
    on:mousedown|preventDefault
    on:click={() => step(1)}
  >+</button>
  {#if suffix}<span class="number-suffix">{suffix}</span>{/if}
</span>
