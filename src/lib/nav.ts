/**
 * 固定导航（我的一天 / 计划内 / 收藏 / 日记 / 记账 / 定时任务 / 工具箱）的目录。
 *
 * 前四个是 `kind:"system"` 的节点（id 与 defaults.ts 的种子节点一致），后三个不是节点：
 * 日记/记账各是一个领域，工具箱是注册表驱动的整页视图（v0.7.5 起两端都有）。
 * 它们共用一套可见性与展示方式配置（`appearance.navItems` / `appearance.navLayout`），
 * 所以 id 必须在这一处对齐。
 */

export type NavItemId = "my-day" | "planned" | "important" | "diary" | "ledger" | "scheduled" | "toolbox";

export type NavLayout = "list" | "grid" | "icons";

/** 默认顺序：与侧栏一直以来的排列一致 */
export const NAV_ITEM_IDS: readonly NavItemId[] = [
  "my-day",
  "planned",
  "important",
  "diary",
  "ledger",
  "scheduled",
  "toolbox"
];

export const NAV_ITEM_LABELS: Record<NavItemId, string> = {
  "my-day": "我的一天",
  planned: "计划内",
  important: "收藏",
  diary: "日记",
  ledger: "记账",
  scheduled: "定时任务",
  toolbox: "工具箱"
};

export const NAV_LAYOUTS: { id: NavLayout; label: string; hint: string }[] = [
  { id: "list", label: "单列", hint: "图标 + 名称，一行一个（默认）" },
  { id: "grid", label: "双列", hint: "图标 + 名称，两列铺开，省一半高度" },
  { id: "icons", label: "只图标", hint: "单行只放图标，名称进悬浮提示" }
];

function isNavItemId(value: unknown): value is NavItemId {
  return typeof value === "string" && (NAV_ITEM_IDS as readonly string[]).includes(value);
}

/** 认得的 id 才留下，去重、保持用户给的顺序；空/非法一律回默认全量。 */
export function normalizeNavItems(raw: unknown): NavItemId[] {
  if (!Array.isArray(raw)) return [...NAV_ITEM_IDS];
  const out: NavItemId[] = [];
  for (const item of raw) {
    if (isNavItemId(item) && !out.includes(item)) out.push(item);
  }
  return out.length > 0 ? out : [...NAV_ITEM_IDS];
}

export function normalizeNavLayout(raw: unknown): NavLayout {
  return raw === "grid" || raw === "icons" ? raw : "list";
}
