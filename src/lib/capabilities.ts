import { get } from "svelte/store";
import { isMobile, hostOs } from "./platform";

const mobile = get(isMobile);

export const isMobilePlatform = mobile;

export const caps = {
  scheduler: !mobile,
  trayLifecycle: !mobile,
  globalShortcuts: !mobile,
  windowZoom: !mobile,
  // Linux 桌面（WSLg/部分 DE）托盘与自绘弹窗不可靠：通知统一走系统通知
  popupNotificationWindow: !mobile && hostOs !== "linux",
  systemNotifications: mobile || hostOs === "linux",
  updateChannel: (mobile ? "apk" : hostOs === "linux" ? "none" : "desktop") as "apk" | "desktop" | "none",
  nativeFileDialogs: !mobile,
  toolbox: mobile,
  desktop: !mobile,
  // 部分 Linux 环境的 WebKitGTK 对 asset 协议子资源根本不发请求（strace 实测
  // 零次文件打开），图像改走 dataURL（与移动端同一路径）；其他平台保持 asset 协议。
  dataUrlImages: hostOs === "linux"
};

// 平台差异收敛在本层：示例路径按宿主 OS 给（Linux 无 .exe 语义）
export const executablePathPlaceholder = hostOs === "linux" ? "/usr/local/bin/tool" : "C:\\Tools\\demo.exe";
