// CodeMirror 6 编辑器装配：Markdown 语法高亮 + 应用主题一致的配色。
// 独立成模块，便于 MarkdownEditorModal 与未来其他编辑场景复用。

import { EditorState, Prec, type Extension } from "@codemirror/state";
import {
  EditorView,
  keymap,
  drawSelection,
  dropCursor,
  placeholder
} from "@codemirror/view";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab
} from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { languages } from "@codemirror/language-data";
import {
  HighlightStyle,
  syntaxHighlighting
} from "@codemirror/language";
import { tags } from "@lezer/highlight";

const mdHighlight = HighlightStyle.define([
  { tag: tags.heading1, color: "#1f2937", fontWeight: "700", fontSize: "1.25em" },
  { tag: tags.heading2, color: "#1f2937", fontWeight: "700", fontSize: "1.15em" },
  { tag: [tags.heading3, tags.heading4], color: "#1f2937", fontWeight: "600" },
  { tag: tags.strong, color: "#111827", fontWeight: "700" },
  { tag: tags.emphasis, color: "#374151", fontStyle: "italic" },
  { tag: tags.strikethrough, textDecoration: "line-through", color: "#6b7280" },
  { tag: tags.link, color: "#2563eb", textDecoration: "underline" },
  { tag: tags.url, color: "#9ca3af" },
  { tag: tags.quote, color: "#6b7280", fontStyle: "italic" },
  { tag: tags.monospace, color: "#b91c1c", backgroundColor: "#f3f4f6", borderRadius: "3px" },
  { tag: [tags.processingInstruction, tags.punctuation], color: "#9ca3af" },
  { tag: tags.list, color: "#2563eb" },
  { tag: tags.contentSeparator, color: "#d1d5db" },
  { tag: tags.labelName, color: "#7c3aed" }
]);

export type EditorHandlers = {
  onSave?: () => void;
  onClose?: () => void;
  onChange?: (text: string) => void;
  onPasteImage?: (file: File, view: EditorView) => void;
  placeholder?: string;
};

export function createMarkdownEditor(
  host: HTMLElement,
  doc: string,
  handlers: EditorHandlers = {}
): EditorView {
  const extensions: Extension[] = [
    history(),
    drawSelection(),
    dropCursor(),
    EditorState.allowMultipleSelections.of(true),
    syntaxHighlighting(mdHighlight),
    // 空列表项上的回车（退一级/顶级清标识）必须压过 lang-markdown 自带的
    // 「回车续列表」——markdown() 在扩展数组里排在前头，它那份 keymap 优先级更高，
    // 不用 Prec.high 就永远轮不到这条。非空行返回 false，续列表照旧。
    Prec.high(keymap.of([{ key: "Enter", run: outdentEmptyListItem }])),
    markdown({ base: markdownLanguage, codeLanguages: languages }),
    EditorView.lineWrapping,
    placeholder(handlers.placeholder ?? ""),
    keymap.of([
      {
        key: "Mod-s",
        preventDefault: true,
        run: () => {
          handlers.onSave?.();
          return true;
        }
      },
      {
        key: "Escape",
        preventDefault: true,
        run: () => {
          handlers.onClose?.();
          return true;
        }
      },
      // 列表里的 Tab 先按列表语义缩进（连标记与编号一起改），不在列表里才落回
      // CodeMirror 的 indentWithTab —— 返回 false 就是「我不管」，后面的绑定接着跑。
      // `indentWithTab` 是个 KeyBinding（Tab → indentMore、Shift-Tab → indentLess），
      // 自己那两条要在它前面，否则列表行永远走不到。
      {
        key: "Tab",
        preventDefault: true,
        run: (view) => indentListItem(view, 1) || Boolean(indentWithTab.run?.(view))
      },
      { key: "Shift-Tab", preventDefault: true, run: (view) => indentListItem(view, -1) },
      indentWithTab,
      ...defaultKeymap,
      ...historyKeymap
    ]),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) {
        handlers.onChange?.(update.state.doc.toString());
      }
    }),
    EditorView.domEventHandlers({
      paste(event, view) {
        if (!handlers.onPasteImage) return;
        const items = event.clipboardData?.items;
        if (!items) return;
        for (const item of items) {
          if (item.type.startsWith("image/")) {
            const file = item.getAsFile();
            if (file) {
              event.preventDefault();
              handlers.onPasteImage(file, view);
            }
            return;
          }
        }
      }
    })
  ];

  // 光标默认落在文末：打开编辑器基本都是为了续写，不是从头改
  const view = new EditorView({
    state: EditorState.create({
      doc,
      extensions,
      selection: { anchor: doc.length }
    }),
    parent: host
  });
  view.dispatch({ scrollIntoView: true });
  return view;
}

/** 整篇替换（预览里点勾选框时用）：内容没变就什么都不做，免得白记一条撤销历史。 */
export function replaceDocument(view: EditorView, text: string): void {
  const current = view.state.doc.toString();
  if (current === text) return;
  view.dispatch({ changes: { from: 0, to: current.length, insert: text } });
}

/** 列表行：`缩进 + 标记 + 可选的 `[ ] ` 勾选框 + 空白`。 */
const LIST_ITEM_RE = /^(\s*)([-*+]|\d+[.)])(\s+)(\[[ xX]\]\s+)?/;

/** 无序标记按层级轮换，缩进一眼能看出嵌套关系。 */
const BULLETS = ["-", "*", "+"];

function bulletFor(indent: number): string {
  return BULLETS[Math.floor(indent / 2) % BULLETS.length];
}

/**
 * 列表语义的缩进 / 反缩进（Tab / Shift-Tab）。
 *
 * CodeMirror 自带的 `indentWithTab` 只会往行首塞两个空格：标记与编号一概不管，
 * 于是缩进去之后编号还是原来的（`1. 2. 3.` 里插一层就变成 `1. 1. 2.`），
 * 无序列表的标记也不变，看不出层级。这里按列表语义整块处理：
 * - 选区里每一行**只要是列表项**就整行缩进两格（反缩进最少回到行首）；
 * - 无序标记按层级轮换成 `-` / `*` / `+`（含 `- [ ]` 这种待办项，勾选框原样保留）；
 * - 整块里的有序列表**重新编号**（同缩进连续的一段从 1 数起）。
 *
 * 选区里一行列表项都没有就返回 false —— 调用方据此落回 CodeMirror 的默认 Tab。
 */
export function indentListItem(view: EditorView, direction: 1 | -1): boolean {
  const state = view.state;
  const range = state.selection.main;
  const first = state.doc.lineAt(range.from).number;
  let last = state.doc.lineAt(range.to).number;
  const caret = range.empty;
  const isContinuation = (text: string): boolean => !LIST_ITEM_RE.test(text) && /^\s+\S/.test(text);
  // 光标（无选区）停在一个列表项上时，把它自己的续行一起纳入——松散列表
  // 「1. 甲 ⏎ 缩进正文」是一个条目，只缩标记行会把条目缩散架。
  if (caret && LIST_ITEM_RE.test(state.doc.line(first).text)) {
    while (last < state.doc.lines && isContinuation(state.doc.line(last + 1).text)) last += 1;
  }
  const changes: Array<{ from: number; to: number; insert: string }> = [];
  let touched = 0;
  // 光标要落在**最后一个被改的行的行尾**（新文档坐标）：缩进是整行前缀改写，
  // 早先把光标钉在 changes.last.from（行首），用户缩进完得重新点到行尾才能续写。
  // 行尾位置 = 原行尾 + 到这一行为止的净位移（前面每行的前缀增删都会挪动它）。
  let cursorAnchor = -1;
  let delta = 0;
  let inItem = false;
  for (let n = first; n <= last; n++) {
    const line = state.doc.line(n);
    const match = LIST_ITEM_RE.exec(line.text);
    if (!match) {
      // 缩进续行：跟着所属条目一起动；其它行（空行/正文）不打断也不处理
      if (inItem && isContinuation(line.text)) {
        const ownIndent = line.text.length - line.text.trimStart().length;
        const removeLen = direction === 1 ? 0 : Math.min(2, ownIndent);
        const insert = direction === 1 ? "  " : "";
        changes.push({ from: line.from, to: line.from + removeLen, insert });
        delta += insert.length - removeLen;
        cursorAnchor = line.to + delta;
        touched += 1;
      } else if (!isContinuation(line.text)) {
        inItem = false;
      }
      continue;
    }
    touched += 1;
    inItem = true;
    const indent = match[1];
    const marker = match[2];
    const ordered = /^\d/.test(marker);
    const nextIndent =
      direction === 1
        ? indent + "  "
        : indent.slice(0, Math.max(0, indent.length - 2));
    // 有序列表的标记交给重新编号那一步改写，这里只动缩进
    const nextMarker = ordered ? marker : bulletFor(nextIndent.length);
    const nextText = nextIndent + nextMarker + match[3] + (match[4] ?? "");
    changes.push({ from: line.from, to: line.from + match[0].length, insert: nextText });
    delta += nextText.length - match[0].length;
    cursorAnchor = line.to + delta;
  }
  if (touched === 0) return false;
  // 行号必须在 dispatch **之前**取：反缩进会把文档变短，旧选区位置随之越界
  const anchorLine = state.doc.lineAt(range.from).number;
  view.dispatch({ changes, selection: { anchor: cursorAnchor }, scrollIntoView: true });
  renumberOrderedList(view, anchorLine);
  return true;
}

/**
 * 空列表项上的回车 = 往左退一级（顶级则整个清掉列表标识），**不是**再换一行。
 * 与主流 markdown 编辑器同口径：连按两次回车就能退出列表。光标留在原行。
 * 有内容的行返回 false，落回 lang-markdown 自己的「续列表/自增编号」。
 */
export function outdentEmptyListItem(view: EditorView): boolean {
  const state = view.state;
  const range = state.selection.main;
  if (!range.empty) return false;
  const line = state.doc.lineAt(range.head);
  const match = LIST_ITEM_RE.exec(line.text);
  if (!match) return false;
  if (line.text.slice(match[0].length).trim() !== "") return false;
  if (match[1].length >= 2) return indentListItem(view, -1);
  // 顶级空项：标识（含 `- [ ] ` 勾选框）整个删掉，行留下、光标停在行首
  view.dispatch({
    changes: { from: line.from, to: line.to, insert: "" },
    selection: { anchor: line.from },
    scrollIntoView: true
  });
  renumberOrderedList(view, line.number);
  return true;
}

/**
 * 把一段连续的有序列表重新编号（每个缩进层级各自从 1 数起）。
 *
 * 从 `lineNumber` 往上/下扩到整个列表块（列表项与缩进续行都算块内，空行断开），
 * 再逐行扫。**计数按缩进层级记在 Map 里**，不是「连续同级才累加」：
 * - 嵌套块结束后回到外层，外层的序号要接着数（`1. 甲 → 嵌套 → 2. 乙`），
 *   早先按「与上一行同缩进才 +1」实现，乙会被错编成 1.；
 * - 松散列表的续行（缩进正文）不打断任何层级的计数；
 * - 回到较浅层级时清掉更深层级的计数（那一段已经结束了）。
 */
function renumberOrderedList(view: EditorView, lineNumber: number): void {
  const state = view.state;
  const isListish = (text: string): boolean => LIST_ITEM_RE.test(text) || /^\s+\S/.test(text);
  let start = Math.min(Math.max(lineNumber, 1), state.doc.lines);
  let end = start;
  while (start > 1 && isListish(state.doc.line(start - 1).text)) start -= 1;
  while (end < state.doc.lines && isListish(state.doc.line(end + 1).text)) end += 1;
  const changes: Array<{ from: number; to: number; insert: string }> = [];
  const counters = new Map<string, number>();
  for (let n = start; n <= end; n++) {
    const line = state.doc.line(n);
    const match = LIST_ITEM_RE.exec(line.text);
    if (!match) continue; // 续行等非列表项行：不动计数
    if (!/^\d/.test(match[2])) continue; // 无序项不参与编号，也不打断计数
    const indent = match[1];
    for (const key of [...counters.keys()]) {
      if (key.length > indent.length) counters.delete(key);
    }
    const next = (counters.get(indent) ?? 0) + 1;
    counters.set(indent, next);
    const text = `${next}.`;
    if (match[2] === text) continue;
    changes.push({ from: line.from + indent.length, to: line.from + indent.length + match[2].length, insert: text });
  }
  if (changes.length === 0) return;
  view.dispatch({ changes });
}

/** 在光标处插入文本（无选区）或替换选区，并把光标移到插入内容之后。 */
export function insertAtCursor(view: EditorView, text: string): void {
  const range = view.state.selection.main;
  view.dispatch({
    changes: { from: range.from, to: range.to, insert: text },
    selection: { anchor: range.from + text.length },
    scrollIntoView: true
  });
  view.focus();
}

/**
 * 用前后标记包裹选区（加粗/斜体/高亮…）。无选区时插入占位文本并选中它；
 * 选区外侧已经是同样的标记时拆掉（再点一次 = 取消）。光标/选区始终落在标记内部。
 */
export function wrapSelection(view: EditorView, before: string, after: string, placeholder: string): void {
  const state = view.state;
  const range = state.selection.main;
  const selected = state.sliceDoc(range.from, range.to);
  const wrapped =
    range.from >= before.length &&
    range.to + after.length <= state.doc.length &&
    state.sliceDoc(range.from - before.length, range.from) === before &&
    state.sliceDoc(range.to, range.to + after.length) === after;
  if (wrapped && selected) {
    view.dispatch({
      changes: [
        { from: range.from - before.length, to: range.from },
        { from: range.to, to: range.to + after.length }
      ],
      selection: { anchor: range.from - before.length, head: range.to - before.length },
      scrollIntoView: true
    });
    view.focus();
    return;
  }
  const inner = selected || placeholder;
  view.dispatch({
    changes: { from: range.from, to: range.to, insert: before + inner + after },
    selection: { anchor: range.from + before.length, head: range.from + before.length + inner.length },
    scrollIntoView: true
  });
  view.focus();
}

/** 标题级别：已是该级别则取消，是别的级别则换掉，没有则加上（逐行处理选区）。
 *  光标落在光标所在行的「# 」之后——留在行首的话用户接着打字会打进标记里。 */
export function setHeading(view: EditorView, level: number): void {
  const state = view.state;
  const range = state.selection.main;
  const prefix = "#".repeat(level) + " ";
  const headLine = state.doc.lineAt(range.head);
  const changes: Array<{ from: number; to: number; insert: string }> = [];
  let headStripped = false;
  for (let n = state.doc.lineAt(range.from).number; n <= state.doc.lineAt(range.to).number; n++) {
    const line = state.doc.line(n);
    const match = /^#{1,6}\s/.exec(line.text);
    if (match && match[0] === prefix) {
      changes.push({ from: line.from, to: line.from + match[0].length, insert: "" });
      if (n === headLine.number) headStripped = true;
    } else if (match) {
      changes.push({ from: line.from, to: line.from + match[0].length, insert: prefix });
    } else {
      changes.push({ from: line.from, to: line.from, insert: prefix });
    }
  }
  // 目标行行首在新文档里的位置 = 原位置 + 它之前各行的净位移
  let delta = 0;
  for (const change of changes) {
    if (change.from >= headLine.from) break;
    delta += change.insert.length - (change.to - change.from);
  }
  const anchor = headLine.from + delta + (headStripped ? 0 : prefix.length);
  view.dispatch({ changes, selection: { anchor }, scrollIntoView: true });
  view.focus();
}

/** 整行前缀开关（checkbox / 无序 / 有序列表）。ordered 按选区内行序递增编号。
 *  光标落在光标所在行的前缀之后——与标题按钮同口径，留在行首接着打字会打进标记里。 */
export function toggleLinePrefix(view: EditorView, prefix: string, ordered = false): void {
  const state = view.state;
  const range = state.selection.main;
  const has = (text: string): boolean => (ordered ? /^\s*\d+[.)]\s/.test(text) : text.startsWith(prefix));
  const strip = (text: string): string => {
    const match = ordered ? /^\s*\d+[.)]\s/.exec(text) : text.startsWith(prefix) ? [prefix] : null;
    return match ? match[0] : "";
  };
  const headLine = state.doc.lineAt(range.head);
  const lines: Array<{ number: number; from: number; text: string }> = [];
  for (let n = state.doc.lineAt(range.from).number; n <= state.doc.lineAt(range.to).number; n++) {
    const line = state.doc.line(n);
    lines.push({ number: n, from: line.from, text: line.text });
  }
  const allHave = lines.every((line) => has(line.text));
  const changes: Array<{ from: number; to: number; insert: string }> = [];
  let headStripped = false;
  let headInsertLen = 0;
  lines.forEach((line, index) => {
    const next = ordered ? index + 1 + ". " : prefix;
    const existing = strip(line.text);
    if (allHave) {
      changes.push({ from: line.from, to: line.from + existing.length, insert: "" });
      if (line.number === headLine.number) headStripped = true;
    } else if (existing) {
      changes.push({ from: line.from, to: line.from + existing.length, insert: next });
    } else {
      changes.push({ from: line.from, to: line.from, insert: next });
    }
    if (line.number === headLine.number && !allHave) headInsertLen = next.length;
  });
  // 目标行行首在新文档里的位置 = 原位置 + 它之前各行的净位移
  let delta = 0;
  for (const change of changes) {
    if (change.from >= headLine.from) break;
    delta += change.insert.length - (change.to - change.from);
  }
  const anchor = headLine.from + delta + (headStripped ? 0 : headInsertLen);
  view.dispatch({ changes, selection: { anchor }, scrollIntoView: true });
  view.focus();
}

/** 超链接：`[选区](url)`，选区落在 url 上方便直接输入地址。 */
export function insertLink(view: EditorView): void {
  const state = view.state;
  const range = state.selection.main;
  const label = state.sliceDoc(range.from, range.to) || "链接文字";
  const insert = "[" + label + "](url)";
  const urlFrom = range.from + label.length + 3;
  view.dispatch({
    changes: { from: range.from, to: range.to, insert },
    selection: { anchor: urlFrom, head: urlFrom + 3 },
    scrollIntoView: true
  });
  view.focus();
}
