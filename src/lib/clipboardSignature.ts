/**
 * 安卓前台剪切板读取的「这次读到了什么」签名（纯逻辑，有单测）。
 *
 * 分工要拎清：这道签名只防「同一次读取被重复提交」（原生变化事件与
 * visibilitychange 可能前后脚各触发一次读取）；**内容级去重是 core 的事**
 * （`ClipboardContent::signature()`：文本 + 图片字节 + 文件名/大小），回声抑制
 * 靠 core 的 last_local/last_remote。
 *
 * 因此这里必须把 Kotlin 桥落下的缓存文件**路径本身**算进来：
 * - 图片/文件每次读取都落在新的 UUID 缓存目录 → 签名必然不同 → 交给 core 按内容判重；
 * - 旧实现图片只算一个 "image" 占位符，连复制两张**不同**的图签名都一样，
 *   第二张在前端就被当成重复静默吞掉——「总是有收不到图」的前端侧根因。
 */
export type NativeClipboardLike = {
  text: string;
  imagePath: string | null;
  files: Array<{ name: string; path: string }>;
};

export function nativeClipboardSignature(content: NativeClipboardLike): string {
  return [
    content.text,
    content.imagePath ?? "",
    ...content.files.map((file) => `${file.name}:${file.path}`)
  ].join("\u0000");
}
