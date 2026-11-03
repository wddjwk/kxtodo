import { describe, expect, it } from "vitest";

import {
  TASK_MENU_DEFAULT_ORDER,
  isTaskMenuSeparator,
  normalizeTaskMenuConfig,
  taskMenuItemLabel,
  visibleTaskMenuIds
} from "../taskMenuConfig";

describe("任务右键菜单配置（v0.9.2）", () => {
  it("没配置过 = 默认顺序，三个截止快捷项默认藏", () => {
    const visible = visibleTaskMenuIds(undefined);
    expect(visible).toEqual([
      "myday",
      "schedule",
      "calendar",
      "important",
      "pin",
      "tags",
      "emoji",
      "move",
      "sep",
      "edit",
      "delete"
    ]);
    expect(visible).not.toContain("due-today");
  });

  it("order 生效、认不出的 id 丢弃、缺的按默认顺序补尾", () => {
    const config = normalizeTaskMenuConfig({
      order: ["edit", "ghost", "myday"],
      hidden: ["pin", "unknown"]
    });
    expect(config.order.slice(0, 3)).toEqual(["edit", "myday", "schedule"]);
    expect(config.order).not.toContain("ghost");
    expect(config.order.length).toBe(TASK_MENU_DEFAULT_ORDER.length);
    expect(config.hidden).toEqual(["pin"]);
  });

  it("hidden 把条目从菜单里滤掉，分隔线也能藏", () => {
    const visible = visibleTaskMenuIds({ hidden: ["sep", "emoji"] });
    expect(visible).not.toContain("sep");
    expect(visible).not.toContain("emoji");
    expect(visible).toContain("edit");
  });

  it("分隔线与名字查得到", () => {
    expect(isTaskMenuSeparator("sep")).toBe(true);
    expect(isTaskMenuSeparator("edit")).toBe(false);
    expect(taskMenuItemLabel("due-tomorrow")).toBe("设置明天截止");
  });
});
