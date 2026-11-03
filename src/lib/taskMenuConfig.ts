/**
 * 任务右键菜单的条目注册表与顺序/显隐解析（v0.9.2）。
 *
 * 菜单本身在 `Workspace.svelte` 里按 `visibleTaskMenuIds()` 渲染；顺序与隐藏项住在
 * `settings.appearance.taskMenu`（`{ order, hidden }`，本机偏好不进同步共享子集），
 * core 只校验形状（`ops_config::expect_task_menu`），**id 白名单只有这一份**。
 *
 * 解析纪律：认不出的 id（老版本数据 / 手改 JSON）直接丢弃，缺的 id 按注册表默认顺序
 * 补到尾部——菜单永远画得出来，不会因为一份坏配置整块消失。
 */

export type TaskMenuItemId =
  | "myday"
  | "schedule"
  | "calendar"
  | "important"
  | "pin"
  | "tags"
  | "emoji"
  | "move"
  | "sep"
  | "edit"
  | "delete"
  | "due-today"
  | "due-tomorrow"
  | "due-dayafter";

export type TaskMenuItemDef = {
  id: TaskMenuItemId;
  /** 管理面板里展示的名字（菜单里的实际文案可能随状态变，如「置顶/取消置顶」） */
  label: string;
  /** 分隔线也是一等条目：能拖、能藏 */
  separator?: boolean;
  /** 默认不展示（需求：三个截止快捷项默认藏着，用户自己开） */
  defaultHidden?: boolean;
};

export const TASK_MENU_ITEMS: TaskMenuItemDef[] = [
  { id: "myday", label: "我的一天" },
  { id: "schedule", label: "日期与提醒" },
  { id: "calendar", label: "添加到日程" },
  { id: "important", label: "收藏" },
  { id: "pin", label: "置顶" },
  { id: "tags", label: "标签" },
  { id: "emoji", label: "添加表情" },
  { id: "move", label: "移动到" },
  { id: "sep", label: "分隔线", separator: true },
  { id: "edit", label: "编辑" },
  { id: "delete", label: "删除" },
  { id: "due-today", label: "设置今天截止", defaultHidden: true },
  { id: "due-tomorrow", label: "设置明天截止", defaultHidden: true },
  { id: "due-dayafter", label: "设置后天截止", defaultHidden: true }
];

const IDS: TaskMenuItemId[] = TASK_MENU_ITEMS.map((item) => item.id);

export const TASK_MENU_DEFAULT_ORDER: TaskMenuItemId[] = [...IDS];

export type TaskMenuConfig = { order: TaskMenuItemId[]; hidden: TaskMenuItemId[] };

function asIdList(raw: unknown): TaskMenuItemId[] {
  if (!Array.isArray(raw)) return [];
  const out: TaskMenuItemId[] = [];
  for (const item of raw) {
    if (typeof item !== "string") continue;
    const id = item as TaskMenuItemId;
    if (!IDS.includes(id) || out.includes(id)) continue;
    out.push(id);
  }
  return out;
}

/** 把存储值收敛成合法配置：认不出的 id 丢弃、缺的按默认顺序补尾、hidden 只留已知 id。
 *  从没配置过（undefined/null）时 hidden 取注册表的 defaultHidden——「用户显式全显示」
 *  存的是 `hidden: []`（键在），与「没配置」必须分得开。 */
export function normalizeTaskMenuConfig(raw: unknown): TaskMenuConfig {
  if (raw === undefined || raw === null) {
    return {
      order: [...TASK_MENU_DEFAULT_ORDER],
      hidden: TASK_MENU_ITEMS.filter((item) => item.defaultHidden).map((item) => item.id)
    };
  }
  const map = raw as { order?: unknown; hidden?: unknown };
  const ordered = asIdList(map.order);
  for (const id of TASK_MENU_DEFAULT_ORDER) {
    if (!ordered.includes(id)) ordered.push(id);
  }
  const hidden = map.hidden === undefined
    ? TASK_MENU_ITEMS.filter((item) => item.defaultHidden).map((item) => item.id)
    : asIdList(map.hidden).filter((id) => ordered.includes(id));
  return { order: ordered, hidden };
}

/** 菜单实际渲染顺序（含分隔线、含隐藏项——隐藏由 visibleTaskMenuIds 再滤）。 */
export function taskMenuOrder(raw: unknown): TaskMenuItemId[] {
  return normalizeTaskMenuConfig(raw).order;
}

/** 菜单要画出来的条目（按顺序、去掉隐藏项）。 */
export function visibleTaskMenuIds(raw: unknown): TaskMenuItemId[] {
  const { order, hidden } = normalizeTaskMenuConfig(raw);
  return order.filter((id) => !hidden.includes(id));
}

export function taskMenuItemLabel(id: TaskMenuItemId): string {
  return TASK_MENU_ITEMS.find((item) => item.id === id)?.label ?? id;
}

export function isTaskMenuSeparator(id: TaskMenuItemId): boolean {
  return TASK_MENU_ITEMS.find((item) => item.id === id)?.separator === true;
}

/** 默认显隐：注册表里 defaultHidden 的项默认藏，其余默认露。 */
export function isTaskMenuDefaultHidden(id: TaskMenuItemId): boolean {
  return TASK_MENU_ITEMS.find((item) => item.id === id)?.defaultHidden === true;
}
