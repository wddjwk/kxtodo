<script lang="ts">
  /**
   * 统计视图：顶行两个段控（左 = 周期，右 = 侧 支出/收入/结余），
   * 下面一整块白底圆角卡 = 可点周期标签 + 三标签 + 三数额，再往下是趋势与分类占比。
   * 曲线手写 SVG（描边动画），环抽成了 LedgerDonut（钻取面板复用同一份）——
   * 不引图表库，包体积与风格都不值，server 管理台的活动曲线就是先例。
   *
   * **段控是分平台的（v0.7.5）**：桌面「周/月/年/总/自定义 + 支出/收入/结余」；
   * 移动端去掉「周」且文案收短（月/年/总/自定义 + 支/收/结余）——五个词加三个词
   * 在 390px 上必超屏，超出去的不只是段控自己，下面的周期标签与弹层也跟着出界。
   *
   * **侧选「结余」时趋势画三条线**：收入（绿）/ 支出（红）/ 结余=收-支（黑），
   * 纵轴此时按有符号区间自适应（结余可以是负的）。分类占比仍按支出画
   * （占比环的语义是"钱花在哪一类"）。
   *
   * **趋势图可读数**：桌面悬浮、移动端点按——最近的桶上画标记线与圆点，
   * 顶部浮出该桶的日期与各条数值。
   *
   * 排行里点一个大类派发 drill 让上层弹出钻取面板（移动端下半屏、桌面端锚定下拉）。
   */
  import { createEventDispatcher } from "svelte";
  import { ChevronLeft, ChevronRight } from "@lucide/svelte";
  import {
    categoryColor, categoryStats, compactCents, formatCents, statsBounds, statsEntries,
    statsPeriodLabel, statsSeries, shiftWeek, paletteColor, type StatsMode
  } from "../ledger";
  import type { LedgerDonutItem } from "../ledger";
  import { ledgerIcon } from "../ledgerIcons";
  import { fitAmount } from "../fitText";
  import { todayDate } from "../diary";
  import { appSettings, weekStart } from "../stores";
  import { createBackGuard, isMobile } from "../platform";
  import { uiScaleValue } from "../styles";
  import { clampPopoverToViewport } from "../popover";
  import LedgerDonut from "./LedgerDonut.svelte";
  import MonthPopover from "../MonthPopover.svelte";
  import DatePicker from "../DatePicker.svelte";
  import type { LedgerBook, LedgerEntry, LedgerSide } from "../types";
  import type { MonthCursor } from "../diary";

  export let book: LedgerBook;
  export let entries: LedgerEntry[];
  export let cursor: MonthCursor;

  const dispatch = createEventDispatcher<{
    month: MonthCursor;
    drill: {
      categoryId: string;
      side: LedgerSide;
      from: string;
      to: string;
      periodLabel: string;
      anchor: HTMLElement;
    };
  }>();

  /** 段控里的「侧」只剩收支两档；**结余曲线改由图例胶囊切出**（v0.8.4 需求 10） */
  type Side = LedgerSide;
  /** 图上要画的曲线：三枚图例胶囊各自开关，默认 收入 + 支出 亮、结余灰 */
  type LegendKey = "income" | "expense" | "balance";

  const MODES: Array<{ id: StatsMode; label: string }> = [
    { id: "week", label: "周" },
    { id: "month", label: "月" },
    { id: "year", label: "年" },
    { id: "total", label: "总" },
    { id: "custom", label: "自定义" }
  ];
  const SIDE_LABELS: Record<Side, { full: string; short: string }> = {
    expense: { full: "支出", short: "支" },
    income: { full: "收入", short: "收" }
  };
  const SIDES: Side[] = ["expense", "income"];

  let mode: StatsMode = "month";
  let side: Side = "expense";
  let legend: Record<LegendKey, boolean> = { income: true, expense: true, balance: false };
  /** 周周期的锚点日（周一起算那一周）；自定义周期的起止 */
  let weekAnchor = todayDate();
  let customFrom = `${cursor.year}-${(cursor.month + 1).toString().padStart(2, "0")}-01`;
  let customTo = todayDate();
  let popOpen: "" | "month" | "week" | "from" | "to" = "";
  let periodEl: HTMLElement;
  let rootEl: HTMLElement;
  let chartBox: HTMLElement;
  /** 趋势图上正在读数的桶（桌面悬浮 / 移动点按）；null = 没在读 */
  let hoverIndex: number | null = null;

  // 「周」两端都有（v0.8.4 需求 10 把移动端加回来——侧段控少了一档，宽度腾出来了）
  $: modes = MODES;
  $: sideText = (key: Side) => ($isMobile ? SIDE_LABELS[key].short : SIDE_LABELS[key].full);
  $: bounds = statsBounds(entries, mode, cursor, weekAnchor, customFrom, customTo, $weekStart);
  $: periodLabel = statsPeriodLabel(mode, bounds, cursor);
  // 曲线、占比、排行都吃同一个窗口——早前占比拿全量数据配当期汇总，两个数字对不上
  $: rangeEntries = statsEntries(entries, bounds);
  $: series = statsSeries(entries, bounds);
  $: totalIncome = series.reduce((sum, point) => sum + point.income, 0);
  $: totalExpense = series.reduce((sum, point) => sum + point.expense, 0);
  /** 占比环与排行跟着侧段控（支出/收入）走：环的语义是"钱花在哪一类" */
  let catSide: LedgerSide = "expense";
  $: catSide = side;
  $: stats = categoryStats(book, rangeEntries, catSide).filter((item) => item.cents > 0);
  $: statsTotal = stats.reduce((sum, item) => sum + item.cents, 0);
  // 桶类型看键长：month 桶的键是 "2026-09"（7 位），day 桶是 "2026-09-14"。
  // 早前判反了（>7 当 month），day 桶被按月格式化，横轴整排「NaN月」。
  let bucket: "day" | "month" = "day";
  $: bucket = series.length > 0 && series[0].key.length === 7 ? "month" : "day";

  function colorOf(categoryId: string, index: number): string {
    const category = book.categories.find((item) => item.id === categoryId);
    return category?.color || paletteColor(index);
  }

  $: donutItems = stats.slice(0, 12).map<LedgerDonutItem>((item, index) => ({
    id: item.categoryId || "none",
    name: item.name,
    cents: item.cents,
    count: item.count,
    color: colorOf(item.categoryId, index)
  }));

  // --- 曲线几何（纵轴按可见曲线自适应：结余可以是负的） ---
  const W = 680;
  const H = 220;
  const PAD_X = 40;
  const PAD_TOP = 18;
  const PAD_BOTTOM = 28;
  const COLOR_IN = "#2f9e6e";
  const COLOR_OUT = "#e0654f";
  const COLOR_BAL = "#2b3038";
  /**
   * 折线「描边动画」的长度基准：和 `ledger.css` 里 `.ledger-line` 的
   * `stroke-dasharray / stroke-dashoffset` 必须是同一个数。
   *
   * 它通过 SVG 的 `pathLength` 生效——把路径长度**归一化**成这个值，于是
   * dash 图案永远恰好等于「整条路径」，与真实几何长度解耦。不这么做的症状
   * （v0.8.4 修）：点数一多、折线一陡，真实长度超过 dash 常量之后图案会重复
   * （2400 实 + 2400 空），折线中间断掉、末尾画不出来，看着没盖住阴影区域。
   */
  const CHART_DASH_LEN = 2400;

  function balanceOf(point: { income: number; expense: number }): number {
    return point.income - point.expense;
  }

  /** 三条曲线的定义（与图例胶囊一一对应） */
  const LINE_SPECS: Array<{
    key: LegendKey;
    name: string;
    color: string;
    value: (point: { income: number; expense: number }) => number;
  }> = [
    { key: "income", name: "收入", color: COLOR_IN, value: (p) => p.income },
    { key: "expense", name: "支出", color: COLOR_OUT, value: (p) => p.expense },
    { key: "balance", name: "结余", color: COLOR_BAL, value: balanceOf }
  ];

  /** 图上要画的曲线 = 图例开着的那几条（至少留一条，否则点没了就没图可看） */
  $: lines = LINE_SPECS.filter((spec) => legend[spec.key]);

  $: visibleValues = series.flatMap((point) => lines.map((line) => line.value(point)));
  $: hi = Math.max(1, ...visibleValues);
  // 结余可以是负的：画了结余就把 0 以下也纳进纵向范围
  $: lo = legend.balance ? Math.min(0, ...visibleValues) : 0;

  /** 图例开关：点一下就切换；最后一条亮着的不许关（关掉整张图就空了） */
  function toggleLegend(key: LegendKey): void {
    if (legend[key] && lines.length === 1) return;
    legend = { ...legend, [key]: !legend[key] };
    hoverIndex = null;
  }
  $: stepX = series.length > 1 ? (W - PAD_X * 2) / (series.length - 1) : 0;
  function pointX(index: number): number {
    return PAD_X + index * stepX;
  }
  function pointY(cents: number): number {
    const span = hi - lo || 1;
    return H - PAD_BOTTOM - ((cents - lo) / span) * (H - PAD_TOP - PAD_BOTTOM);
  }
  function linePath(value: (point: { income: number; expense: number }) => number): string {
    return series
      .map((point, index) => `${index === 0 ? "M" : "L"}${pointX(index).toFixed(1)},${pointY(value(point)).toFixed(1)}`)
      .join(" ");
  }
  function areaPath(value: (point: { income: number; expense: number }) => number): string {
    if (series.length === 0) return "";
    const base = pointY(Math.max(0, lo));
    return `${linePath(value)} L${pointX(series.length - 1).toFixed(1)},${base.toFixed(1)} L${pointX(0).toFixed(1)},${base.toFixed(1)} Z`;
  }
  function axisLabel(value: number): string {
    // 金额单位是分：1 万元 = 1_000_000 分
    if (Math.abs(value) >= 1_000_000) return `${(value / 1_000_000).toFixed(1).replace(/\.0$/, "")}万`;
    return compactCents(value);
  }
  $: axisValues = [hi, (hi + lo) / 2, lo];
  $: tickIndexes = series
    .map((_, index) => index)
    .filter((index) => index % Math.max(1, Math.ceil(series.length / 8)) === 0);

  /** 悬浮/点按读数：把视口横坐标换算成最近的桶 */
  function readAt(clientX: number): void {
    if (!chartBox || series.length === 0) return;
    const rect = chartBox.getBoundingClientRect();
    if (rect.width <= 0) return;
    const viewX = ((clientX - rect.left) / rect.width) * W;
    const index = Math.round((viewX - PAD_X) / (stepX || 1));
    hoverIndex = Math.min(series.length - 1, Math.max(0, index));
  }

  $: hoverPoint = hoverIndex !== null ? series[hoverIndex] : null;
  $: hoverTitle = hoverPoint
    ? bucket === "month"
      ? `${hoverPoint.key.slice(0, 4)}年${Number(hoverPoint.key.slice(5, 7))}月`
      : hoverPoint.key.replaceAll("-", "/")
    : "";
  $: tipLeft = hoverIndex !== null ? Math.min(88, Math.max(12, (pointX(hoverIndex) / W) * 100)) : 0;

  // 周期 / 自定义区间的日期气泡：返回键直接收掉
  const backGuard = createBackGuard();
  $: backGuard(popOpen !== "", () => (popOpen = ""));

  function step(delta: number): void {
    popOpen = "";
    if (mode === "week") {
      weekAnchor = shiftWeek(weekAnchor, delta);
      return;
    }
    if (mode === "month") {
      const date = new Date(cursor.year, cursor.month + delta, 1);
      dispatch("month", { year: date.getFullYear(), month: date.getMonth() });
      return;
    }
    if (mode === "year") {
      dispatch("month", { year: cursor.year + delta, month: cursor.month });
    }
  }

  function pickPeriod(next: { year: number; month: number }): void {
    popOpen = "";
    dispatch("month", { year: next.year, month: next.month });
  }

  function switchMode(next: StatsMode): void {
    mode = next;
    popOpen = "";
  }

  function switchSide(next: Side): void {
    side = next;
  }

  /** 自定义起止的 DatePicker 浮层：窄屏上锚点靠右时会伸出屏幕，开出来后收进视口 */
  function toggleCustomPop(which: "from" | "to"): void {
    popOpen = popOpen === which ? "" : which;
    if (popOpen) {
      void clampPopoverToViewport(
        rootEl,
        uiScaleValue($appSettings.appearance.uiScale),
        `.ledger-custom-field.open .ledger-pop`
      );
    }
  }

  function drill(item: { categoryId: string }, anchor: HTMLElement): void {
    popOpen = "";
    dispatch("drill", { categoryId: item.categoryId, side: catSide, from: bounds.from, to: bounds.to, periodLabel, anchor });
  }

  function slash(date: string): string {
    return date.replaceAll("-", "/");
  }

  // 周期/侧/区间/图例一变，图整个重画：读数标记不能留在旧位置
  $: chartKey = `${mode}-${side}-${bounds.from}-${bounds.to}-${legend.income ? "i" : ""}${legend.expense ? "e" : ""}${legend.balance ? "b" : ""}`;
  $: if (chartKey) hoverIndex = null;

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.isComposing || event.keyCode === 229) return;
    if (popOpen) {
      event.preventDefault();
      event.stopPropagation();
      popOpen = "";
    }
  }

  /**
   * 自定义起止 / 周锚点的 DatePicker 浮层：点别处要自己收起来，不能非要再点一下日期
   * （月份浮层自己有这套逻辑，见 MonthPopover）。捕获阶段监听——面板内很多地方对
   * pointerdown 做了 stopPropagation。
   */
  function closeOnOutside(event: PointerEvent): void {
    if (popOpen !== "from" && popOpen !== "to" && popOpen !== "week") return;
    const node = event.target as Node | null;
    if (!node || !rootEl) return;
    const zones =
      popOpen === "week"
        ? [".ledger-stats-period", ".ledger-week-pop"]
        : [".ledger-custom-field.open"];
    for (const selector of zones) {
      const zone = rootEl.querySelector(selector);
      if (zone?.contains(node)) return;
    }
    popOpen = "";
  }
</script>

<svelte:window on:keydown={handleKeydown} on:pointerdown|capture={closeOnOutside} />

<div class="ledger-stats" bind:this={rootEl}>
  <div class="ledger-stats-bar">
    <div class="ledger-segmented" role="tablist" aria-label="统计范围">
      {#each modes as item (item.id)}
        <button type="button" role="tab" class:active={mode === item.id} on:click|stopPropagation={() => switchMode(item.id)}>
          {item.label}
        </button>
      {/each}
    </div>
    <div class="ledger-segmented ledger-side-switch" role="tablist" aria-label="收支两侧">
      {#each SIDES as key (key)}
        <button type="button" role="tab" class:active={side === key} on:click|stopPropagation={() => switchSide(key)}>
          {sideText(key)}
        </button>
      {/each}
    </div>
  </div>

  <section class="ledger-panel ledger-summary-card">
    <div class="ledger-stats-period">
      {#if mode === "custom"}
        <span class="ledger-custom-field" class:open={popOpen === "from"}>
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_noninteractive_element_to_interactive_role -->
          <strong
            role="button"
            tabindex="0"
            title="起始日期"
            on:click|stopPropagation={() => toggleCustomPop("from")}
          >{slash(bounds.from)}</strong>
          {#if popOpen === "from"}
            <div class="ledger-pop date">
              <DatePicker
                value={customFrom}
                on:select={(event) => {
                  customFrom = event.detail;
                  if (customTo < customFrom) customTo = customFrom;
                  popOpen = "";
                }}
                on:close={() => (popOpen = "")}
              />
            </div>
          {/if}
        </span>
        <span class="ledger-custom-sep">-</span>
        <span class="ledger-custom-field" class:open={popOpen === "to"}>
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_noninteractive_element_to_interactive_role -->
          <strong
            role="button"
            tabindex="0"
            title="结束日期"
            on:click|stopPropagation={() => toggleCustomPop("to")}
          >{slash(bounds.to)}</strong>
          {#if popOpen === "to"}
            <div class="ledger-pop date">
              <DatePicker
                value={customTo}
                on:select={(event) => {
                  customTo = event.detail;
                  if (customFrom > customTo) customFrom = customTo;
                  popOpen = "";
                }}
                on:close={() => (popOpen = "")}
              />
            </div>
          {/if}
        </span>
      {:else}
        {#if mode !== "total"}
          <button type="button" aria-label="上一段" on:click|stopPropagation={() => step(-1)}><ChevronLeft size={17} /></button>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions a11y_no_noninteractive_element_to_interactive_role -->
        <strong
          bind:this={periodEl}
          class="month-pop-anchor"
          class:pickable={mode === "month" || mode === "year" || mode === "week"}
          role="button"
          tabindex="0"
          title={mode === "total" ? "全部流水的跨度" : "点击直接选周期"}
          on:click|stopPropagation={() => {
            if (mode === "month" || mode === "year") popOpen = popOpen === "month" ? "" : "month";
            else if (mode === "week") popOpen = popOpen === "week" ? "" : "week";
          }}
        >{periodLabel}</strong>
        {#if mode !== "total"}
          <button type="button" aria-label="下一段" on:click|stopPropagation={() => step(1)}><ChevronRight size={17} /></button>
        {/if}
      {/if}
    </div>

    {#if mode === "month" || mode === "year"}
      <MonthPopover
        open={popOpen === "month"}
        anchor={periodEl}
        year={cursor.year}
        month={cursor.month}
        mode={mode === "year" ? "year" : "month"}
        onSelect={pickPeriod}
        onClose={() => (popOpen = "")}
      />
    {:else if mode === "week"}
      {#if popOpen === "week"}
        <div class="ledger-pop date ledger-week-pop">
          <DatePicker
            value={weekAnchor}
            on:select={(event) => {
              weekAnchor = event.detail;
              popOpen = "";
            }}
            on:close={() => (popOpen = "")}
          />
        </div>
      {/if}
    {/if}

    <div class="ledger-summary-labels">
      <span>支出</span>
      <span>收入</span>
      <span>结余</span>
    </div>
    <div class="ledger-summary">
      <strong class="out" use:fitAmount={totalExpense}>{formatCents(totalExpense)}</strong>
      <strong class="in" use:fitAmount={totalIncome}>{formatCents(totalIncome)}</strong>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <strong class:out={totalIncome - totalExpense < 0} use:fitAmount={totalIncome - totalExpense}>
        {formatCents(totalIncome - totalExpense)}
      </strong>
    </div>
  </section>

  <section class="ledger-panel">
    <header class="ledger-panel-head">
      <h2>收支趋势</h2>
      <!-- 图例即开关（v0.8.4 需求 10）：三枚胶囊各自显隐一条曲线，
           默认收入 + 支出亮、结余灰。 -->
      <span class="ledger-legend" role="group" aria-label="显示哪些曲线">
        {#each LINE_SPECS as spec (spec.key)}
          <button
            type="button"
            class:on={legend[spec.key]}
            class:bal={spec.key === "balance"}
            title={`${legend[spec.key] ? "隐藏" : "显示"}${spec.name}曲线`}
            aria-pressed={legend[spec.key]}
            on:click|stopPropagation={() => toggleLegend(spec.key)}
          >
            <!-- 亮着才给 --dot（空值时 var() 取不到回退色，色块会整个透明） -->
            <i style={legend[spec.key] ? `--dot: ${spec.color}` : ""}></i>{spec.name}
          </button>
        {/each}
      </span>
    </header>
    {#key chartKey}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <!-- svelte-ignore a11y_click_events_have_key_events -->
      <div
        class="ledger-chart-box"
        bind:this={chartBox}
        on:mousemove={(event) => { if (!$isMobile) readAt(event.clientX); }}
        on:mouseleave={() => { if (!$isMobile) hoverIndex = null; }}
        on:click|stopPropagation={(event) => { if ($isMobile) readAt(event.clientX); }}
      >
        <svg class="ledger-line-chart" viewBox="0 0 {W} {H}" role="img" aria-label="收支趋势曲线">
          {#each axisValues as value (value)}
            <line class="ledger-chart-grid" x1={PAD_X} x2={W - PAD_X} y1={pointY(value)} y2={pointY(value)} />
            <text class="ledger-chart-axis" x={PAD_X - 8} y={pointY(value) + 4} text-anchor="end">{axisLabel(value)}</text>
          {/each}
          {#if lo < 0}
            <line class="ledger-chart-zero" x1={PAD_X} x2={W - PAD_X} y1={pointY(0)} y2={pointY(0)} />
          {/if}
          {#if lines.length === 1}
            <path class="ledger-line-area {lines[0].key === "income" ? "in" : "out"}" d={areaPath(lines[0].value)} />
          {/if}
          {#each lines as line (line.key)}
            <path
              class="ledger-line {line.key === "income" ? "in" : line.key === "expense" ? "out" : "bal"}"
              pathLength={CHART_DASH_LEN}
              d={linePath(line.value)}
            />
          {/each}
          {#if hoverPoint}
            <line class="ledger-chart-marker" x1={pointX(hoverIndex ?? 0)} x2={pointX(hoverIndex ?? 0)} y1={PAD_TOP - 6} y2={H - PAD_BOTTOM} />
            {#each lines as line (line.key)}
              <circle
                class="ledger-chart-dot"
                cx={pointX(hoverIndex ?? 0)}
                cy={pointY(line.value(hoverPoint))}
                r="3.6"
                fill={line.color}
              />
            {/each}
          {/if}
          {#each tickIndexes as index (index)}
            <text class="ledger-chart-axis" x={pointX(index)} y={H - 8} text-anchor="middle">
              {bucket === "day" ? Number(series[index].key.slice(8)) : `${Number(series[index].key.slice(5, 7))}月`}
            </text>
          {/each}
        </svg>
        {#if hoverPoint}
          <div class="ledger-chart-tip" style="left: {tipLeft}%">
            <strong>{hoverTitle}</strong>
            {#each lines as line (line.key)}
              <span><i style="background: {line.color}"></i>{line.name} {formatCents(line.value(hoverPoint))}</span>
            {/each}
          </div>
        {/if}
      </div>
    {/key}
  </section>

  <section class="ledger-panel">
    <header class="ledger-panel-head">
      <h2>分类占比</h2>
    </header>

    {#if stats.length === 0}
      <div class="ledger-day-empty">{periodLabel}还没有{catSide === "expense" ? "支出" : "收入"}记录。</div>
    {:else}
      <div class="ledger-proportion">
        <div class="ledger-donut-wrap">
          <LedgerDonut items={donutItems} total={statsTotal} totalLabel={catSide === "expense" ? "总支出" : "总收入"} />
        </div>

        <ul class="ledger-rank">
          {#each stats as item (item.categoryId || "none")}
            {@const category = book.categories.find((entry) => entry.id === item.categoryId)}
            {@const icon = ledgerIcon(category?.icon, catSide === "income" ? "Banknote" : "Package")}
            {@const color = categoryColor(book, category)}
            <li>
              <button
                type="button"
                class="ledger-rank-head"
                title="查看这一类的二级分类与账单明细"
                on:click|stopPropagation={(event) => drill(item, event.currentTarget)}
              >
                <span class="ledger-rank-icon" style="--cat: {color}; background: {color}">
                  <svelte:component this={icon} size={15} />
                </span>
                <span class="ledger-rank-text">
                  <strong>{item.name}</strong>
                  <em>{item.count} 笔 · {item.percent}%</em>
                  <span class="ledger-rank-track"><i style="width: {item.percent}%; background: {color}"></i></span>
                </span>
                <b class="ledger-rank-amount">{formatCents(item.cents)}</b>
                <ChevronRight class="ledger-rank-go" size={16} />
              </button>
            </li>
          {/each}
        </ul>
      </div>
    {/if}
  </section>
</div>
