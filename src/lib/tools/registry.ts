/**
 * 工具注册表（v0.7.5 的工具机制架构）：ToolboxView 只是壳（列表 + 子视图宿主），
 * 每个工具是这里的一条注册项。分层原则——
 *
 * - **目录**（`catalog.ts`）：id / 名称 / 描述。固定导航也要认工具 id，但它不需要
 *   图标与组件，所以目录单独一层，nav 只 import 目录、不 import 这里。
 * - **可用性**（`available`）：按平台能力（caps）决定这个工具在这一端露不露出。
 *   缺省 = 全平台可用；桌面独有 / 移动独有的工具在这里各自把关。
 * - **实现**（`load`）：动态 import 的子视图组件。两端同实现就共用一个组件。
 * - **壳**（ToolboxView.svelte）：只负责「列表 → 打开子视图 → 返回」，
 *   不认识任何具体工具。新增工具 = 目录加一项 + 这里补图标与 load + 写一个组件。
 *
 * 目录里还没有实现的工具**不出现在 TOOLS 里**（flat 时滤掉）：组件文件还没写的那一段
 * 时间里，列表与固定导航都不该露出一个点进去白屏的条目。
 *
 * 子视图是纯组件内部状态，不占历史栈层级（移动端返回键由壳所在的整页层承担）。
 */
import { ArrowLeftRight, Banknote, Dice5 } from "@lucide/svelte";
import type { Component } from "svelte";
import { TOOL_CATALOG, type ToolId } from "./catalog";
import ScratchpadIcon from "./ScratchpadIcon.svelte";

export type ToolDefinition = {
  id: ToolId;
  name: string;
  desc: string;
  icon: Component;
  /** 平台可用性；缺省全平台可用 */
  available?: () => boolean;
  /** 子视图组件（懒加载，点开才进 bundle 执行） */
  load: () => Promise<{ default: Component }>;
};

const implementations: Partial<
  Record<ToolId, { icon: Component; load: () => Promise<{ default: Component }> }>
> = {
  random: {
    icon: Dice5,
    load: () => import("./RandomTool.svelte")
  },
  rmb: {
    icon: Banknote,
    load: () => import("./RmbTool.svelte")
  },
  scratchpad: {
    icon: ScratchpadIcon,
    load: () => import("./ScratchpadTool.svelte")
  },
  transfer: {
    icon: ArrowLeftRight,
    load: () => import("./TransferTool.svelte")
  }
};

export const TOOLS: ToolDefinition[] = TOOL_CATALOG.flatMap((entry) => {
  const implementation = implementations[entry.id];
  return implementation ? [{ ...entry, ...implementation }] : [];
});

/** 当前平台可用的工具（列表页就按这个画） */
export function availableTools(): ToolDefinition[] {
  return TOOLS.filter((tool) => (tool.available ? tool.available() : true));
}

export function toolById(id: string): ToolDefinition | undefined {
  return TOOLS.find((tool) => tool.id === id);
}

/**
 * 子视图 chunk 的加载缓存（v0.8.4）：同一个工具只发一次请求。
 * **失败不留缓存**——再点一次就是一次真正的重试，否则用户看到的是「返回后再进还是失败」。
 * 壳用 `{#await loadToolModule(tool)}` 直接把它挂到模板上，不再自己管加载状态。
 */
const pendingLoads = new Map<ToolId, Promise<{ default: Component }>>();

export function loadToolModule(tool: ToolDefinition): Promise<{ default: Component }> {
  let pending = pendingLoads.get(tool.id);
  if (!pending) {
    pending = tool.load().catch((error: unknown) => {
      pendingLoads.delete(tool.id);
      throw error;
    });
    pendingLoads.set(tool.id, pending);
  }
  return pending;
}
