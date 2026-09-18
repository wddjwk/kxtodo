/**
 * 固定导航（我的一天 / 计划内 / 收藏 / 日记 / 记账 / 定时任务 / 工具箱 / 钉住的工具）的目录。
 *
 * 前四个是 `kind:"system"` 的节点（id 与 defaults.ts 的种子节点一致），日记/记账各是一个
 * 领域，工具箱是注册表驱动的整页视图（v0.7.5 起两端都有）。v0.8.3 起**工具也可以钉进来**
 * （`tool:<id>`）：右键工具箱条目「固定此工具」，就在这一块多一行直达该工具子视图。
 * 它们共用一套可见性与展示方式配置（`appearance.navItems` / `appearance.navLayout`），
 * 所以 id 必须在这一处对齐。
 */
import { TOOL_CATALOG, type ToolId } from "./tools/catalog";

export type NavBaseId =
  | "my-day"
  | "planned"
  | "important"
  | "diary"
  | "ledger"
  | "scheduled"
  | "toolbox";

/** 钉住的工具行：`tool:rmb` 这种。模板字符串类型让调用方拼错 id 编译期就报。 */
export type NavToolId = `tool:${ToolId}`;

export type NavItemId = NavBaseId | NavToolId;

export type NavLayout = "list" | "grid" | "icons";

/** 默认顺序：与侧栏一直以来的排列一致（不含钉住的工具——那是用户自己加的） */
export const NAV_ITEM_IDS: readonly NavBaseId[] = [
  "my-day",
  "planned",
  "important",
  "diary",
  "ledger",
  "scheduled",
  "toolbox"
];

export const NAV_ITEM_LABELS: Record<NavBaseId, string> = {
  "my-day": "我的一天",
  planned: "计划内",
  important: "收藏",
  diary: "日记",
  ledger: "记账",
  scheduled: "定时任务",
  toolbox: "工具箱"
};

export function navToolId(toolId: ToolId): NavToolId {
  return `tool:${toolId}`;
}

export function navIdToolId(id: NavItemId): ToolId | null {
  return id.startsWith("tool:") ? (id.slice("tool:".length) as ToolId) : null;
}

/** 展示名：基础行查表，工具行查工具目录（目录里没有 = 非法 id，调用方自己兜底）。 */
export function navItemLabel(id: NavItemId): string {
  const tool = navIdToolId(id);
  if (tool !== null) {
    return TOOL_CATALOG.find((entry) => entry.id === tool)?.name ?? tool;
  }
  return NAV_ITEM_LABELS[id as NavBaseId];
}

export const NAV_LAYOUTS: { id: NavLayout; label: string; hint: string }[] = [
  { id: "list", label: "单列", hint: "图标 + 名称，一行一个（默认）" },
  { id: "grid", label: "双列", hint: "图标 + 名称，两列铺开，省一半高度" },
  { id: "icons", label: "只图标", hint: "单行只放图标，名称进悬浮提示" }
];

export function isNavItemId(value: unknown): value is NavItemId {
  if (typeof value !== "string") return false;
  if ((NAV_ITEM_IDS as readonly string[]).includes(value)) return true;
  const tool = value.startsWith("tool:") ? value.slice("tool:".length) : null;
  return tool !== null && TOOL_CATALOG.some((entry) => entry.id === tool);
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
