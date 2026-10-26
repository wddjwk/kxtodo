import { describe, expect, it } from "vitest";
import { nativeClipboardSignature } from "../clipboardSignature";

const base = { text: "", imagePath: null as string | null, files: [] as Array<{ name: string; path: string }> };

describe("nativeClipboardSignature", () => {
  it("两张不同的图必须得到不同的签名（回归：连复制两张图第二张被前端吞掉）", () => {
    // Kotlin 桥每次读取都把图片落进新的 UUID 缓存目录，路径不同 = 不同的读取结果；
    // 旧实现图片只算 "image" 占位符，两次复制的签名一样，第二张永远发不出去。
    const first = nativeClipboardSignature({
      ...base,
      imagePath: "/data/cache/clipboard-out/aaaa/clipboard.png"
    });
    const second = nativeClipboardSignature({
      ...base,
      imagePath: "/data/cache/clipboard-out/bbbb/clipboard.png"
    });
    expect(first).not.toBe(second);
  });

  it("同一次读取结果签名稳定", () => {
    const content = {
      text: "同一段文字",
      imagePath: "/data/cache/clipboard-out/x/clipboard.png",
      files: [{ name: "a.txt", path: "/data/cache/clipboard-out/x/a.txt" }]
    };
    expect(nativeClipboardSignature(content)).toBe(nativeClipboardSignature(content));
  });

  it("文本变化会改变签名", () => {
    expect(nativeClipboardSignature({ ...base, text: "a" })).not.toBe(
      nativeClipboardSignature({ ...base, text: "b" })
    );
  });

  it("文件项按 名字:路径 参与签名，数量变化也会改变签名", () => {
    const one = nativeClipboardSignature({
      ...base,
      files: [{ name: "a.txt", path: "/p/a.txt" }]
    });
    const two = nativeClipboardSignature({
      ...base,
      files: [
        { name: "a.txt", path: "/p/a.txt" },
        { name: "b.txt", path: "/p/b.txt" }
      ]
    });
    expect(one).not.toBe(two);
  });

  it("空剪切板的签名是确定的（基线读取也走同一函数）", () => {
    expect(nativeClipboardSignature(base)).toBe("\u0000");
  });
});
