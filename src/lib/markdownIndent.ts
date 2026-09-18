/**
 * 行首空白的保留（需求 14）：用户会写
 *
 *     1. xx
 *     ⇥1.1 xx
 *
 * marked 把这种行当段落的 lazy continuation——换行折叠成空格、缩进整个丢掉，
 * 渲染出来三行挤成一行且顶格。这里在进 marked **之前**把行首空白换成渲染得出来的东西：
 *
 * - 缩进换成**不间断空格**（普通空格在 HTML 里会被折叠）；
 * - 若这一行本来要跟上一行连成一段（上一行不是空行 / 块级开头），给**上一行**补一个
 *   markdown 硬换行（行尾两个空格），让它们仍在同一个块里但各占一行。
 *
 * 为什么不是在本行前面插 `<br>`：那样这一行就不再是上一行的续行，marked 会把列表提前
 * 收口，缩进行掉到列表外面另起一段（`<ol><li>xx</li></ol><p><br>…`），段首那个
 * `<br>` 还多出一行空白。补在上一行行尾则两全：列表结构不动，行也分开了。
 *
 * **不动的行**（marked 本来就理解它们，动了才是破坏）：围栏代码块内、缩进代码块
 * （≥4 空格且无 tab）、空行，以及缩进属于**结构**的行——列表标记（`- x` / `1. x`）、
 * 引用（`> x`）、标题（`# x`）、分隔线、setext 下划线。这些行的缩进 marked 会渲染成
 * 真正的嵌套列表或块级元素，不是「丢掉的空白」。
 * 纯函数、不 import marked/DOMPurify，为的是能跑 node 单测。
 */

const NBSP = "\u00A0";
const HARD_BREAK = "  ";
const TAB_SPACES = 4;

/** 「上一行自己就是一个新块的开始」：标题 / 围栏 / 分隔线 / 表格 / HTML 块。
 *  这些行后面的缩进行会自成一段，不需要硬换行。
 *  注意**列表项与引用不算**：它们的续行在 marked 眼里是同一段的 lazy continuation，
 *  换行会被折叠成空格——恰恰需要硬换行才能各占一行。 */
function startsFreshBlock(line: string): boolean {
  const trimmed = line.trim();
  if (trimmed === "") return true;
  return /^(#{1,6}\s|```|~~~|---+\s*$|\|.*\||<\/?[a-zA-Z])/.test(trimmed);
}

/** 这一行的缩进是 markdown 结构，marked 自己会渲染出来，别插手。
 *  `1.1 xx` 不算列表标记（`.` 后面不是空格），正是用户手写的大纲层级。 */
function indentIsStructure(line: string): boolean {
  return /^(>\s?|[-*+]\s|\d{1,9}[.)]\s|#{1,6}\s|[-*_]{3,}\s*$|=+\s*$)/.test(line);
}

/** 行首空白宽度（tab 按 4 空格算，与 markdown 的缩进语义一致）。
 *  **纯计数**，不含「算不算缩进代码块」的判断——那份判断在这里的 `indentWidth`，
 *  计数本身 `markdownTasks.ts` 也要用（判断任务项是否落在代码块里）。 */
export function indentWidthOf(line: string): number {
  let width = 0;
  for (const ch of line) {
    if (ch === " ") width += 1;
    else if (ch === "\t") width += TAB_SPACES;
    else break;
  }
  return width;
}

/** 这一行的缩进该不该当成「渲染得出来的空白」处理。
 *  纯空格缩进 ≥4 是 markdown 的缩进代码块，返回 -1 不处理；
 *  **带 tab 的缩进不限层数**：tab 在进 marked 之前就被换掉，不会触发代码块语义，
 *  而用户的大纲恰恰是「每层一个 tab」敲出来的。 */
function indentWidth(line: string): number {
  const body = line.trimStart();
  const width = indentWidthOf(line);
  // `trim()` 而不是 `body === ""`：CRLF 的「空行」是 `"\r"`，trimStart 剥不掉它，
  // 判不出来就会把段落之间的空行换成「三个不间断空格 + \r」——两段被并成一段
  if (line.trim() === "") return -1; // 空行
  if (width >= 4 && !line.slice(0, line.length - body.length).includes("\t")) return -1;
  return width;
}

/** 上一行能不能补硬换行：本身是围栏、或已经有硬换行（行尾两空格 / 反斜杠）的就不补。 */
function canTakeHardBreak(line: string): boolean {
  if (/^\s*(`{3,}|~{3,})/.test(line)) return false;
  return !/( {2,}|\\)$/.test(line);
}

export function preserveLeadingIndent(source: string): string {
  if (!/^[ \t]+\S/m.test(source)) return source; // 绝大多数卡片没有缩进行，早退
  const lines = source.split("\n");
  const out: string[] = [];
  let inFence = false;
  let fenceMark = "";
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];
    const fenceMatch = /^\s*(`{3,}|~{3,})/.exec(line);
    if (fenceMatch) {
      if (!inFence) {
        inFence = true;
        fenceMark = fenceMatch[1][0];
      } else if (fenceMatch[1][0] === fenceMark) {
        inFence = false;
      }
      out.push(line);
      continue;
    }
    if (inFence) {
      out.push(line);
      continue;
    }
    const body = line.trimStart();
    const width = indentWidth(line);
    if (width <= 0 || indentIsStructure(body)) {
      out.push(line);
      continue;
    }
    const previous = index > 0 ? lines[index - 1] : "";
    if (!startsFreshBlock(previous) && canTakeHardBreak(previous)) out[index - 1] += HARD_BREAK;
    out.push(NBSP.repeat(width) + body);
  }
  return out.join("\n");
}
