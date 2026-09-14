/**
 * 工具注册表（v0.7.5 的工具机制架构）：ToolboxView 只是壳（列表 + 子视图宿主），
 * 每个工具是这里的一条注册项。分层原则——
 *
 * - **可用性**（`available`）：按平台能力（caps）决定这个工具在这一端露不露出。
 *   缺省 = 全平台可用；桌面独有 / 移动独有的工具在这里各自把关。
 * - **实现**（`load`）：动态 import 的子视图组件。同一功能两端实现不同时，
 *   注册两条同 id 前缀的项、各自 available 分平台即可（壳不感知差异）；
 *   两端同实现就共用一个组件（随机数这种纯前端工具）。
 * - **壳**（ToolboxView.svelte）：只负责「列表 → 打开子视图 → 返回」，
 *   不认识任何具体工具。新增工具 = 往 TOOLS 加一项 + 写一个组件，壳与样式零改动。
 *
 * 子视图是纯组件内部状态，不占历史栈层级（移动端返回键由壳所在的整页层承担）。
 */
import { Dice5 } from "@lucide/svelte";
import type { Component } from "svelte";

export type ToolDefinition = {
  id: string;
  name: string;
  desc: string;
  icon: Component;
  /** 平台可用性；缺省全平台可用 */
  available?: () => boolean;
  /** 子视图组件（懒加载，点开才进 bundle 执行） */
  load: () => Promise<{ default: Component }>;
};

export const TOOLS: ToolDefinition[] = [
  {
    id: "random",
    name: "随机数生成",
    desc: "在指定范围内生成随机整数",
    icon: Dice5,
    load: () => import("./RandomTool.svelte")
  }
];

/** 当前平台可用的工具（列表页就按这个画） */
export function availableTools(): ToolDefinition[] {
  return TOOLS.filter((tool) => (tool.available ? tool.available() : true));
}
