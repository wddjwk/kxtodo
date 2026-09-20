import { describe, expect, it } from "vitest";
import {
  baseName, dirName, filePick, folderPick, groupByRoot, isAbsoluteRel, isFileItem, isTextItem, joinPath, spoolPick,
  splitPathTail, textPick
} from "../transferManifest";

describe("baseName / dirName", () => {
  it("认两种分隔符", () => {
    expect(baseName(String.raw`C:\Users\a\照片.jpg`)).toBe("照片.jpg");
    expect(baseName("/home/user/照片.jpg")).toBe("照片.jpg");
    expect(dirName(String.raw`C:\Users\a\照片.jpg`)).toBe(String.raw`C:\Users\a`);
    expect(dirName("/home/user/照片.jpg")).toBe("/home/user");
  });

  it("盘根与 POSIX 根都保留成可用的 root", () => {
    expect(dirName(String.raw`C:\照片.jpg`)).toBe("C:\\");
    expect(dirName("/照片.jpg")).toBe("/");
  });

  it("没有分隔符时回落 `.`；结尾多余分隔符不算文件名", () => {
    expect(dirName("照片.jpg")).toBe(".");
    expect(baseName("/home/user/文件夹/")).toBe("文件夹");
  });
});

describe("三个来源的 rel 归一（需求 5.1 验收 3）", () => {
  it("桌面选文件：rel 是文件名、root 是所在目录", () => {
    const item = filePick(String.raw`C:\Users\a\Pictures\照片.jpg`, 1024);
    expect(item.rel).toBe("照片.jpg");
    expect(item.root).toBe(String.raw`C:\Users\a\Pictures`);
    expect(isAbsoluteRel(item.rel)).toBe(false);
  });

  it("spool（移动端 webview 选文件）：rel 是文件名、root 是 outbox", () => {
    const item = spoolPick("/data/user/0/app/files/runtime/transfer-outbox", "video.mp4", 2048);
    expect(item.rel).toBe("video.mp4");
    expect(item.root).toBe("/data/user/0/app/files/runtime/transfer-outbox");
    expect(isAbsoluteRel(item.rel)).toBe(false);
  });

  it("剪贴板图片：有 root 的剪贴板项是文件而不是文本", () => {
    const item = spoolPick("/outbox", "剪贴板图片-123.png", 64, "clipboard");
    expect(item.rel).toBe("剪贴板图片-123.png");
    expect(isFileItem(item)).toBe(true);
    expect(isTextItem(item)).toBe(false);
  });

  it("拖放与文件夹：文件夹项自成 root", () => {
    const folder = folderPick(String.raw`D:\资料\项目`, 999);
    expect(folder.rel).toBe(String.raw`D:\资料\项目`);
    expect(folder.root).toBe(folder.rel);
    expect(isFileItem(folder)).toBe(false);
    expect(isTextItem(folder)).toBe(false);
  });

  it("文本项没有 root，正文住在 rel 里", () => {
    const typed = textPick("你好：世界");
    expect(isTextItem(typed)).toBe(true);
    expect(typed.root).toBeNull();
    const clip = textPick("/etc/passwd 的权限", "clipboard");
    expect(isTextItem(clip)).toBe(true);
  });

  it("绝对路径判定与 core tripwire 同口径", () => {
    expect(isAbsoluteRel(String.raw`C:\tmp\a.txt`)).toBe(true);
    expect(isAbsoluteRel(String.raw`\\server\share\a.txt`)).toBe(true);
    expect(isAbsoluteRel("/home/user/a.txt")).toBe(true);
    expect(isAbsoluteRel("照片.jpg")).toBe(false);
    expect(isAbsoluteRel("sub/照片.jpg")).toBe(false);
    expect(isAbsoluteRel(String.raw`sub\照片.jpg`)).toBe(false);
  });
});

describe("groupByRoot：跨目录多选拆成多次发送（需求 5.6）", () => {
  it("同目录合并、跨目录分开，保持首次出现顺序", () => {
    const files = [
      filePick(String.raw`C:\a\1.txt`, 1),
      filePick(String.raw`C:\b\2.txt`, 2),
      filePick(String.raw`C:\a\3.txt`, 3)
    ];
    const groups = groupByRoot(files);
    expect(groups.map(([root]) => root)).toEqual([String.raw`C:\a`, String.raw`C:\b`]);
    expect(groups[0][1].map((item) => item.rel)).toEqual(["1.txt", "3.txt"]);
    expect(groups[1][1].map((item) => item.rel)).toEqual(["2.txt"]);
  });

  it("桌面文件 + 剪贴板图片混选：两个 root，两条发送", () => {
    const files = [filePick(String.raw`C:\桌面\报告.pdf`, 10), spoolPick("/outbox", "剪贴板图片-1.png", 20, "clipboard")];
    const groups = groupByRoot(files);
    expect(groups).toHaveLength(2);
    for (const [, items] of groups) {
      for (const item of items) expect(isAbsoluteRel(item.rel)).toBe(false);
    }
  });
});

describe("接收路径的拼接与显示（需求 5.4）", () => {
  it("joinPath：分隔符跟着目录走，末尾有分隔符不重复", () => {
    expect(joinPath(String.raw`C:\Users\a\Downloads`, "照片.jpg")).toBe(String.raw`C:\Users\a\Downloads\照片.jpg`);
    expect(joinPath("/storage/emulated/0/Download", "照片.jpg")).toBe("/storage/emulated/0/Download/照片.jpg");
    expect(joinPath("/storage/emulated/0/Download/", "照片.jpg")).toBe("/storage/emulated/0/Download/照片.jpg");
    expect(joinPath("", "照片.jpg")).toBe("照片.jpg");
  });

  it("splitPathTail：头段含分隔符、尾段是最后一段（中间省略用）", () => {
    expect(splitPathTail(String.raw`C:\Users\a\Downloads`)).toEqual({ head: "C:\\Users\\a\\", tail: "Downloads" });
    expect(splitPathTail("/storage/emulated/0/Download")).toEqual({ head: "/storage/emulated/0/", tail: "Download" });
    expect(splitPathTail("Download")).toEqual({ head: "", tail: "Download" });
    // 末尾多余分隔符不算最后一段
    expect(splitPathTail("/a/b/")).toEqual({ head: "/a/", tail: "b" });
  });
});
