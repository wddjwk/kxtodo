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
  html: string;
  imagePath: string | null;
  files: Array<{ name: string; path: string }>;
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
      html?: string;
      imagePath?: string | null;
      files?: Array<{ name?: string; path?: string }>;
    };
    if (parsed.error) return null;
    return {
      text: typeof parsed.text === "string" ? parsed.text : "",
      html: typeof parsed.html === "string" ? parsed.html : "",
      imagePath: typeof parsed.imagePath === "string" && parsed.imagePath ? parsed.imagePath : null,
      files: (parsed.files ?? [])
        .filter((file): file is { name: string; path: string } =>
          typeof file?.name === "string" && typeof file?.path === "string" && file.path.length > 0
        )
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
        html: content.html,
        imagePath: content.imagePath ?? "",
        files: content.files.filter((file) => Boolean(file.path))
      })
    );
  } catch (error) {
    return String(error);
  }
}
