/**
 * 传输事件 → 文案的映射（v0.8.6 需求 5.3/5.4）。
 *
 * 这两条规则都是被用户实测打出来的：失败文案一刀切「对方离线了」吞掉了本地真实原因；
 * 历史里「被拒绝」与「失败」必须分得开。
 */
import { describe, expect, it } from "vitest";
import { historyStatusStyle, transferErrorText } from "../transferEvents";

describe("transferErrorText", () => {
  it("连接断开 → 前缀「对方离线了」并带上真实原因", () => {
    const text = transferErrorText("TRANSFER_CONNECTION_LOST", "传输帧读取失败：connection closed");
    expect(text.startsWith("对方离线了（")).toBe(true);
    expect(text).toContain("传输帧读取失败");
  });

  it("本地错误原样透出，绝不冒充「对方离线」（需求 5.4.3）", () => {
    expect(transferErrorText("TRANSFER_PATH_UNSAFE", "传输路径越界：C:\\evil")).toBe("传输路径越界：C:\\evil");
    expect(transferErrorText("TRANSFER_NAME_EXHAUSTED", "同名文件太多")).toBe("同名文件太多");
    expect(transferErrorText("IO_ERROR", "保存写入失败：磁盘满")).toBe("保存写入失败：磁盘满");
  });

  it("被拒绝有专门的文案（发送侧卡片要说清是对方拒了）", () => {
    expect(transferErrorText("TRANSFER_REJECTED", "对方拒绝了这次传输")).toBe("接收方拒绝了这次传输");
  });

  it("没有 message 时给一句兜底", () => {
    expect(transferErrorText(undefined, "")).toBe("传输失败");
    expect(transferErrorText("WHATEVER", "   ")).toBe("传输失败");
  });
});

describe("historyStatusStyle", () => {
  it("四种结局各有色调与图形，rejected 与 failed 分得开", () => {
    expect(historyStatusStyle("done")).toEqual({ tone: "ok", glyph: "✓", label: "完成" });
    expect(historyStatusStyle("text")).toEqual({ tone: "text", glyph: "✉", label: "文本消息" });
    expect(historyStatusStyle("rejected")).toEqual({ tone: "rejected", glyph: "⊘", label: "已拒绝" });
    expect(historyStatusStyle("failed")).toEqual({ tone: "bad", glyph: "✕", label: "失败" });
    expect(historyStatusStyle("cancelled").tone).toBe("bad");
  });
});
