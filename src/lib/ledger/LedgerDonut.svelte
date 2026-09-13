<script lang="ts">
  /**
   * 分类占比环（手写 SVG，不引图表库）：统计视图与分类钻取面板共用一份，
   * 免得两处的引线、防叠字、点选放大各写一遍还长得不一样。
   *
   * 交互：点一片沿中角拉出来加粗、环心换成它的名字/金额/占比笔数，再点回总额。
   * 引线标签只给占比够大的分类画——小切片的名字挤在一起反而读不出来，列表里有全量。
   */
  import { compactCents } from "../ledger";
  import type { LedgerDonutItem } from "../ledger";

  export let items: LedgerDonutItem[];
  export let total: number;
  export let totalLabel: string;

  const CX = 230;
  const CY = 130;
  const R = 72;
  const RING = 26;
  const CIRC = 2 * Math.PI * R;
  const LABEL_MIN_FRACTION = 0.045;
  const LABEL_GAP = 17;

  /** 点中的那一片；数据一换（换月份/换收支侧/换分类）就取消，别留着高亮一个不在环上的分类 */
  let focusId = "";
  let signature = "";
  $: nextSignature = items.map((item) => `${item.id}:${item.cents}`).join("|") + `#${total}`;
  $: if (nextSignature !== signature) {
    signature = nextSignature;
    focusId = "";
  }

  $: segments = (() => {
    let offset = 0;
    return items.slice(0, 12).map((item) => {
      const fraction = total > 0 ? item.cents / total : 0;
      // -90° 起算：第一片从正上方开始，顺时针
      const mid = (offset + fraction / 2) * 2 * Math.PI - Math.PI / 2;
      const segment = {
        ...item,
        fraction,
        offset,
        mid,
        // 选中时沿中角挪出去的方向分量（交给 CSS transform，属性 transform 不做过渡）
        dx: 10 * Math.cos(mid),
        dy: 10 * Math.sin(mid)
      };
      offset += fraction;
      return segment;
    });
  })();

  $: focused = segments.find((segment) => segment.id === focusId) ?? null;
  $: centerLabel = focused ? focused.name : totalLabel;
  $: centerValue = compactCents(focused ? focused.cents : total);
  $: centerNote = focused ? `${Math.round(focused.fraction * 100)}% · ${focused.count} 笔` : "";

  /** 引线标签：先按切片中角算拐点，再把同一侧上下挨太近的名字推开（否则叠字）。 */
  $: labels = (() => {
    type Label = {
      id: string; text: string; x1: number; y1: number; x2: number; y2: number;
      x3: number; y3: number; right: boolean;
    };
    const candidates: Label[] = [];
    for (const segment of segments) {
      if (segment.fraction < LABEL_MIN_FRACTION) continue;
      const cos = Math.cos(segment.mid);
      const sin = Math.sin(segment.mid);
      const right = cos >= 0;
      const x2 = CX + (R + RING / 2 + 14) * cos;
      const y2 = CY + (R + RING / 2 + 14) * sin;
      candidates.push({
        id: segment.id,
        text: `${segment.name.length > 8 ? `${segment.name.slice(0, 8)}…` : segment.name} ${Math.round(segment.fraction * 100)}%`,
        x1: CX + (R + RING / 2 + 3) * cos,
        y1: CY + (R + RING / 2 + 3) * sin,
        x2,
        y2,
        x3: right ? CX + R + RING / 2 + 44 : CX - R - RING / 2 - 44,
        y3: y2,
        right
      });
    }
    for (const side of [true, false]) {
      const group = candidates.filter((item) => item.right === side).sort((a, b) => a.y2 - b.y2);
      let previous = -Number.MAX_VALUE;
      for (const item of group) {
        item.y3 = Math.max(item.y2, previous + LABEL_GAP);
        previous = item.y3;
      }
      // 推开后可能超出画布下缘：整组往上收回
      const overflow = previous + 14 - 260;
      if (overflow > 0) {
        for (const item of group) item.y3 = Math.max(16, item.y3 - overflow);
      }
    }
    return candidates;
  })();

  function toggleFocus(id: string): void {
    focusId = focusId === id ? "" : id;
  }
</script>

<svg class="ledger-donut" viewBox="0 0 460 260" role="img" aria-label="分类占比环">
  <circle class="ledger-donut-track" cx={CX} cy={CY} r={R} fill="none" stroke-width={RING} />
  {#each segments as segment (segment.id)}
    <g
      class="ledger-donut-slice"
      class:focus={segment.id === focusId}
      style="--dx: {segment.dx.toFixed(2)}px; --dy: {segment.dy.toFixed(2)}px"
    >
      <circle
        cx={CX}
        cy={CY}
        r={R}
        fill="none"
        stroke={segment.color}
        stroke-width={RING}
        stroke-dasharray="{(segment.fraction * CIRC).toFixed(2)} {CIRC.toFixed(2)}"
        stroke-dashoffset={(-segment.offset * CIRC).toFixed(2)}
        transform="rotate(-90 {CX} {CY})"
        role="button"
        tabindex="0"
        aria-label="{segment.name} {compactCents(segment.cents)}，点击在环心显示这一类"
        on:click|stopPropagation={() => toggleFocus(segment.id)}
        on:keydown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            toggleFocus(segment.id);
          }
        }}
      />
    </g>
  {/each}
  {#each labels as label (label.id)}
    <polyline
      class="ledger-donut-leader"
      points="{label.x1.toFixed(1)},{label.y1.toFixed(1)} {label.x2.toFixed(1)},{label.y2.toFixed(1)} {label.x3.toFixed(1)},{label.y3.toFixed(1)}"
    />
    <text
      class="ledger-donut-tag"
      x={label.right ? label.x3 + 5 : label.x3 - 5}
      y={label.y3 + 4}
      text-anchor={label.right ? "start" : "end"}
    >{label.text}</text>
  {/each}
  <text class="ledger-donut-label" x={CX} y={focused ? CY - 18 : CY - 8} text-anchor="middle">{centerLabel}</text>
  <text class="ledger-donut-value" x={CX} y={focused ? CY + 12 : CY + 18} text-anchor="middle">{centerValue}</text>
  {#if centerNote}
    <text class="ledger-donut-note" x={CX} y={CY + 34} text-anchor="middle">{centerNote}</text>
  {/if}
</svg>
