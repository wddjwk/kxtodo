// CodeMirror 6 编辑器装配：Markdown 语法高亮 + 应用主题一致的配色。
// 独立成模块，便于 MarkdownEditorModal 与未来其他编辑场景复用。

import { EditorState, type Extension } from "@codemirror/state";
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

  return new EditorView({
    state: EditorState.create({ doc, extensions }),
    parent: host
  });
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
