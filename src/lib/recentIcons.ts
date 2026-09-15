/**
 * 「常用图标」的最近使用记录（本机 UI 状态，不进同步）：图标选择器每选中一个
 * ——不管是表情还是简笔画——就把它挪到队首。选择器顶部那块「常用图标」按这个
 * 顺序排列，最多两行。存 localStorage 就够，它跟其它本机偏好一个待遇。
 */
const STORAGE_KEY = "kxtodo-recent-icons";
/** 最多两行（桌面选择器一行 10 格上下，16 个足够铺满两行且不溢出） */
const MAX_RECENT = 16;

export function loadRecentIcons(): string[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((item): item is string => typeof item === "string" && item.length > 0).slice(0, MAX_RECENT);
  } catch {
    return [];
  }
}

export function rememberIcon(value: string): void {
  const icon = value.trim();
  if (!icon) return;
  const next = [icon, ...loadRecentIcons().filter((item) => item !== icon)].slice(0, MAX_RECENT);
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
  } catch {
    // 存不下就算了：常用图标只是便利，不该弄失败任何一次选择
  }
}
