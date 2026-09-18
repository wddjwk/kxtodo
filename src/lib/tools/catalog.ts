/**
 * 工具目录（v0.8.3 从 registry 里拆出来）：只有 id / 名称 / 描述，**不含图标与组件**。
 *
 * 为什么要拆：固定导航（`nav.ts`）也要认得工具 id（「固定此工具」把工具钉进侧栏），
 * 而它不需要图标与懒加载组件。让 nav 直接 import registry 会把 lucide 图标与
 * 所有工具的 chunk 拉进首屏链——目录这一层就是为了解耦而存在的。
 */

export type ToolId = "random" | "rmb" | "scratchpad" | "transfer";

export type ToolCatalogEntry = {
  id: ToolId;
  name: string;
  desc: string;
};

export const TOOL_CATALOG: readonly ToolCatalogEntry[] = [
  {
    id: "random",
    name: "随机数生成",
    desc: "在指定范围内生成随机整数"
  },
  {
    id: "rmb",
    name: "人民币大小写",
    desc: "金额与财务大写互转（壹仟贰佰叁拾肆元伍角陆分 ⇄ 1234.56）"
  },
  {
    id: "scratchpad",
    name: "草稿纸",
    desc: "一块随手记的纯文本便签，自动保存，不做任何渲染"
  },
  {
    id: "transfer",
    name: "文件传输助手",
    desc: "两台设备凭同一句口令互传文件与文件夹（打洞直连，实时进度）"
  }
];

export function isToolId(value: unknown): value is ToolId {
  return TOOL_CATALOG.some((entry) => entry.id === value);
}

export function toolCatalogEntry(id: string): ToolCatalogEntry | undefined {
  return TOOL_CATALOG.find((entry) => entry.id === id);
}
