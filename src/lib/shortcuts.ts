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

export function shortcutLabel(combo: string): string {
  return combo.replace(/\+/g, " + ");
}
