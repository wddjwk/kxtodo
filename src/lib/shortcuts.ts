import type { ShortcutBinding } from "./types";

const modifierAliases = new Map([
  ["CTRL", "CONTROL"],
  ["CONTROL", "CONTROL"],
  ["CMD", "META"],
  ["COMMAND", "META"],
  ["OPTION", "ALT"]
]);

function normalizePart(part: string): string {
  const upper = part.trim().toUpperCase();
  return modifierAliases.get(upper) ?? upper;
}

export function matchesShortcut(event: KeyboardEvent, binding: ShortcutBinding): boolean {
  const parts = binding.combo
    .split("+")
    .map(normalizePart)
    .filter(Boolean);

  const expectedKey = parts.find((part) => !["CONTROL", "SHIFT", "ALT", "META"].includes(part));

  if (!expectedKey) {
    return false;
  }

  const pressedKey = event.key.length === 1 ? event.key.toUpperCase() : event.key.toUpperCase();

  return (
    Boolean(event.ctrlKey) === parts.includes("CONTROL") &&
    Boolean(event.shiftKey) === parts.includes("SHIFT") &&
    Boolean(event.altKey) === parts.includes("ALT") &&
    Boolean(event.metaKey) === parts.includes("META") &&
    pressedKey === expectedKey
  );
}

export function shortcutLabel(combo: string): string {
  return combo.replace(/\+/g, " + ");
}
