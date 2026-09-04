import { get } from "svelte/store";
import { isMobile } from "./platform";

const mobile = get(isMobile);

export const isMobilePlatform = mobile;

export const caps = {
  scheduler: !mobile,
  trayLifecycle: !mobile,
  globalShortcuts: !mobile,
  windowZoom: !mobile,
  popupNotificationWindow: !mobile,
  systemNotifications: mobile,
  updateChannel: (mobile ? "apk" : "desktop") as "apk" | "desktop",
  nativeFileDialogs: !mobile,
  toolbox: mobile,
  desktop: !mobile
};
