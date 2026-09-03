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

/** Kotlin JS 桥（仅 Android WebView 注入）：同步方法，返回 "" 表示成功，否则为错误信息。 */
interface Window {
  kxtodoAndroid?: {
    installApk(path: string): string;
    shareText(filename: string, mime: string, text: string): string;
  };
}
