import { get, writable } from "svelte/store";

/**
 * Mobile detection is intentionally user-agent based so the Windows desktop
 * experience is never affected (the desktop window enforces a large min-width).
 * Only Android / iOS web views flip the app into the stacked mobile layout.
 */
function detectMobile(): boolean {
  if (typeof navigator === "undefined") {
    return false;
  }
  const ua = navigator.userAgent || "";
  return /Android|iPhone|iPad|iPod/i.test(ua);
}

export const isMobile = writable(detectMobile());

/**
 * Microsoft To-Do style mobile navigation: the app opens on the category list
 * and tapping an entry pushes the content view. The back button returns here.
 */
export type MobileView = "list" | "content";

export const mobileView = writable<MobileView>("list");

export function showMobileContent(): void {
  if (!get(isMobile)) {
    return;
  }
  mobileView.set("content");
  // Push a history entry so the Android hardware back button returns to the
  // category list (via popstate) instead of exiting the app.
  if (typeof history !== "undefined" && history.state?.mobileView !== "content") {
    history.pushState({ mobileView: "content" }, "");
  }
}

export function showMobileList(): void {
  if (get(isMobile) && typeof history !== "undefined" && history.state?.mobileView === "content") {
    // Let popstate drive the state change so browser history stays in sync.
    history.back();
    return;
  }
  mobileView.set("list");
}

if (typeof window !== "undefined") {
  window.addEventListener("popstate", () => {
    if (get(isMobile)) {
      mobileView.set("list");
    }
  });
}
