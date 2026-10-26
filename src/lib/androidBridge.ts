/**
 * Android Kotlin 桥的薄封装（v0.8.9）。
 *
 * 主 frame token 由 MainActivity 用 evaluateJavascript 注入；应用内链接预览里的
 * 外部 iframe 跨源拿不到它，因此即使 addJavascriptInterface 注入到了 iframe，
 * 也调不动读剪切板/装 APK/分享文件这些方法。
 */
import type { ClipboardRemoteContent } from "./backend";

export function androidToken(): string {
  return typeof window === "undefined" ? "" : window.__kxtodoAndroidToken ?? "";
}

export function androidBridge() {
  return typeof window === "undefined" ? undefined : window.kxtodoAndroid;
}

export type NativeClipboard = {
  text: string;
  imagePath: string | null;
  files: Array<{ name: string; path: string }>;
  /** 读不出来的剪切板项数（流打不开等）；>0 时前端必须提示，绝不静默丢 */
  dropped: number;
};

/** 读当前剪切板。桥没就绪/未授权/平台不支持时返回 null。 */
export function readNativeClipboard(): NativeClipboard | null {
  const bridge = androidBridge();
  const token = androidToken();
  if (!bridge?.readClipboard || !token) return null;
  try {
    const raw = bridge.readClipboard(token);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as {
      error?: string;
      text?: string;
      imagePath?: string | null;
      files?: Array<{ name?: string; path?: string }>;
      dropped?: number;
    };
    if (parsed.error) return null;
    return {
      text: typeof parsed.text === "string" ? parsed.text : "",
      imagePath: typeof parsed.imagePath === "string" && parsed.imagePath ? parsed.imagePath : null,
      files: (parsed.files ?? [])
        .filter((file): file is { name: string; path: string } =>
          typeof file?.name === "string" && typeof file?.path === "string" && file.path.length > 0
        ),
      dropped: Number.isInteger(parsed.dropped) && (parsed.dropped as number) > 0
        ? (parsed.dropped as number)
        : 0
    };
  } catch {
    return null;
  }
}

/** 把远端内容写进 Android 剪切板。返回 "" 表示成功。 */
export function writeNativeClipboard(content: ClipboardRemoteContent): string {
  const bridge = androidBridge();
  const token = androidToken();
  if (!bridge?.writeClipboard || !token) return "剪切板桥未就绪";
  try {
    return bridge.writeClipboard(
      token,
      JSON.stringify({
        text: content.text,
        imagePath: content.imagePath ?? "",
        files: content.files.filter((file) => Boolean(file.path))
      })
    );
  } catch (error) {
    return String(error);
  }
}

export type KeepAliveState = {
  enabled: boolean;
  running: boolean;
  notice: string;
  notificationsAllowed: boolean;
};

/**
 * 后台常驻（v0.9.0）：把前台服务对齐到设置。
 *
 * 非安卓（桥不存在 / 桌面 / 浏览器 dev）时是**空操作并返回 ""**——调用方不必自己判平台，
 * 这样「设置变了就 reconcile」可以写成一条无条件的响应式语句。
 */
export function setNativeKeepAlive(enabled: boolean, notice: string): string {
  const bridge = androidBridge();
  const token = androidToken();
  if (!bridge?.setKeepAlive || !token) return "";
  try {
    return bridge.setKeepAlive(token, enabled, notice) ?? "";
  } catch (error) {
    return String(error);
  }
}

/** 常驻状态；桥不可用时返回 null（设置页据此决定要不要显示「未生效」提示）。 */
export function readNativeKeepAlive(): KeepAliveState | null {
  const bridge = androidBridge();
  const token = androidToken();
  if (!bridge?.keepAliveState || !token) return null;
  try {
    const parsed = JSON.parse(bridge.keepAliveState(token) ?? "null") as Partial<KeepAliveState> & {
      error?: string;
    };
    if (parsed?.error) return null;
    return {
      enabled: parsed?.enabled === true,
      running: parsed?.running === true,
      notice: typeof parsed?.notice === "string" ? parsed.notice : "",
      notificationsAllowed: parsed?.notificationsAllowed !== false
    };
  } catch {
    return null;
  }
}

/** 打开保存目录；返回 "" 表示成功，非空为错误文案（没有文件管理器之类）。 */
export function openNativeFolder(path: string): string {
  const bridge = androidBridge();
  const token = androidToken();
  if (!bridge?.openFolder || !token) return "剪切板桥未就绪";
  try {
    return bridge.openFolder(token, path) ?? "";
  } catch (error) {
    return String(error);
  }
}

/** 收到的图片另存一份进相册（Pictures/kxtodo）；返回 "" 表示成功。 */
export function exportNativeImageToGallery(path: string): string {
  const bridge = androidBridge();
  const token = androidToken();
  if (!bridge?.exportImageToGallery || !token) return "";
  try {
    return bridge.exportImageToGallery(token, path) ?? "";
  } catch (error) {
    return String(error);
  }
}
