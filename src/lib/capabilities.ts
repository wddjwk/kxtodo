import { get } from "svelte/store";
import { isMobile, hostOs } from "./platform";

const mobile = get(isMobile);

export const isMobilePlatform = mobile;

export const caps = {
  // 调度引擎在移动端同样进程内跑（v0.8.3：任务提醒与定时通知都要它）。
  // 跑不了的动作类型（脚本 / 外部程序 / 条件探针）由 core 的
  // ops_schedule::ensure_platform_supported 明确拒绝，界面侧也只给「通知」这一种。
  scheduler: true,
  trayLifecycle: !mobile,
  globalShortcuts: !mobile,
  windowZoom: !mobile,
  // Linux 桌面（WSLg/部分 DE）托盘与自绘弹窗不可靠：通知统一走系统通知
  popupNotificationWindow: !mobile && hostOs !== "linux",
  systemNotifications: mobile || hostOs === "linux",
  // 全平台应用内更新：Android 走 APK 安装器（apk）；桌面（Windows/Linux）统一“下载新制品→替换→重启”（desktop）。
  updateChannel: (mobile ? "apk" : "desktop") as "apk" | "desktop",
  nativeFileDialogs: !mobile,
  // Runtime presence is checked in backend.ts; UA-only browser previews keep the iframe.
  linkPreview: mobile ? "custom-tabs" as const : "webview" as const,
  renderedLinkMetadata: !mobile,
  // 工具箱整页两端都有（v0.7.5）：单个工具的平台差异在 tools/registry.ts 的 available 里表达
  toolbox: true,
  desktop: !mobile,
  // 图像走 dataURL 的两类环境（asset 协议取不到子资源）：
  // - 部分 Linux 的 WebKitGTK 对 asset 协议子资源根本不发请求（strace 实测零次文件打开）；
  // - Android WebView 拿不到 http://asset.localhost/ 子资源（markdown 只见语法不出图）。
  // dataURL 路径由 Rust 直接读文件转 base64，不依赖任何 webview 协议实现。
  dataUrlImages: hostOs === "linux" || mobile
};

// 平台差异收敛在本层：示例路径按宿主 OS 给（Linux 无 .exe 语义）
export const executablePathPlaceholder = hostOs === "linux" ? "/usr/local/bin/tool" : "C:\\Tools\\demo.exe";
