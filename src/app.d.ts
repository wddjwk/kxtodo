declare namespace svelteHTML {
  interface IntrinsicElements {
    "emoji-picker": {
      class?: string;
      "data-source"?: string;
      locale?: string;
      "on:emoji-click"?: (event: CustomEvent<{ unicode: string }>) => void;
      onemojiclick?: (event: CustomEvent<{ unicode: string }>) => void;
    };
  }
}

declare module "*.png" {
  const src: string;
  export default src;
}

/** Kotlin JS 桥（仅 Android WebView 注入）：同步方法，返回 "" 表示成功，否则为错误信息。
 * 第一个参数是主 frame token：Android 的 addJavascriptInterface 也会注入 iframe，
 * 远端网页拿不到父窗口里的 token，就不能读剪切板/分享文件/安装 APK。 */
interface Window {
  __kxtodoAndroidToken?: string;
  kxtodoAndroid?: {
    installApk(token: string, path: string): string;
    shareText(token: string, filename: string, mime: string, text: string): string;
    /** 分享一个已存在于应用缓存目录里的文件（日记导出的 zip） */
    shareFile(token: string, path: string, mime: string): string;
    readClipboard(token: string): string;
    writeClipboard(token: string, payload: string): string;
  };
}
