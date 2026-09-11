export function matchesShortcut(event: KeyboardEvent, shortcut: string): boolean {
  const parts = shortcut
    .split("+")
    .map((part) => part.trim().toLowerCase())
    .filter(Boolean);
  const key = parts.find((part) => !["ctrl", "control", "cmd", "meta", "shift", "alt", "option"].includes(part));
  if (!key) {
    return false;
  }

  const ctrlExpected = parts.includes("ctrl") || parts.includes("control");
  const metaExpected = parts.includes("cmd") || parts.includes("meta");
  const shiftExpected = parts.includes("shift");
  const altExpected = parts.includes("alt") || parts.includes("option");
  const normalizedEventKey = event.key.length === 1 ? event.key.toLowerCase() : event.key.toLowerCase().replace("arrow", "");

  return (
    event.ctrlKey === ctrlExpected &&
    event.metaKey === metaExpected &&
    event.shiftKey === shiftExpected &&
    event.altKey === altExpected &&
    normalizedEventKey === key
  );
}

/**
 * 浮层里输入框的 keydown：吞掉全局快捷键（F5 同步、Ctrl+, 设置、数字键…），
 * 但**放行 Escape**——两段式关闭（先收浮层/表单，再关面板）靠 window 上那个
 * Escape 处理器。写 `on:keydown|stopPropagation` 会把 Escape 一起吃掉，
 * 表现是「在输入框里按 Esc 毫无反应」。
 */
export function fieldKeydown(event: KeyboardEvent): void {
  if (event.key !== "Escape") event.stopPropagation();
}

