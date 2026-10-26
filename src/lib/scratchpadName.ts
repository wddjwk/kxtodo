/**
 * 草稿纸标签页的「内容派生默认名」（v0.9.1 需求 18）——纯逻辑，node 单测。
 *
 * 规则：取正文第一行有内容可取的文本，剥掉 markdown 噪音（标题井号、列表符、
 * 任务框、引用符、围栏反引号、强调符；链接留文字、图片整个去掉），再按**宽度**
 * 截断（CJK 等全角字符按两个半角单位算，不是 slice(n) 按字符数截）。
 * 正文没有可取内容时返回空串——调用方（ScratchpadTool）回落到纯数字默认名。
 *
 * 宽度预算的推导（全部是 CSS 布局像素，见 toolbox.css / mobile.css 的
 * `.scratchpad-tab`）：标签页最窄形态 = 移动端 min-width 76px（border-box），
 * 减左右 padding 16px、边框 2px、名字与关闭叉的 gap 5px、关闭叉定宽占位 16px，
 * 名字分到 37px；标签页字号 = calc(var(--font-control) - 4px)，默认
 * --font-control 18px → 14px。以「半角字宽」（≈ fontSize / 2 = 7px）为单位：
 * floor(37 / 7) = 5 个单位 ≈ 2 个全角字或 5 个半角字母——最小宽度下不靠
 * CSS 省略号也放得下。用户调大字号时预算会相应超出，由 `.scratchpad-tab-name`
 * 的 text-overflow: ellipsis 兜底（那是设置改变后的事，不是默认形态）。
 */

/** 名字的宽度预算，单位是「半角字宽」（全角字符记 2、其余记 1）。推导见文件头。 */
export const TAB_NAME_WIDTH_BUDGET = 5;

/** 全角 / 宽字符（East Asian Wide/Fullwidth 的常用段 + emoji）记 2 个单位，其余记 1。 */
export function charWidthUnits(char: string): number {
  const cp = char.codePointAt(0) ?? 0;
  const wide =
    (cp >= 0x1100 && cp <= 0x115f) || // 韩文字母（初声）
    (cp >= 0x2e80 && cp <= 0x303e) || // CJK 部首、康熙部首、CJK 标点
    (cp >= 0x3041 && cp <= 0x33ff) || // 假名、韩文兼容字母、CJK 兼容
    (cp >= 0x3400 && cp <= 0x4dbf) || // CJK 扩展 A
    (cp >= 0x4e00 && cp <= 0x9fff) || // CJK 统一表意文字
    (cp >= 0xa000 && cp <= 0xa4cf) || // 彝文
    (cp >= 0xac00 && cp <= 0xd7a3) || // 韩文音节
    (cp >= 0xf900 && cp <= 0xfaff) || // CJK 兼容表意文字
    (cp >= 0xfe30 && cp <= 0xfe4f) || // CJK 兼容形式
    (cp >= 0xff00 && cp <= 0xff60) || // 全角形式
    (cp >= 0xffe0 && cp <= 0xffe6) || // 全角符号
    (cp >= 0x1f300 && cp <= 0x1f64f) || // emoji（多数字体按全角渲染）
    (cp >= 0x1f900 && cp <= 0x1f9ff) || // 补充 emoji
    (cp >= 0x20000 && cp <= 0x3fffd); // CJK 扩展 B 及以后
  return wide ? 2 : 1;
}

/**
 * 按宽度预算截断（逐码点累加单位数，放不下就停）。**不加省略号**——省略号本身
 * 也占宽，加上就超预算了；「还有更多」由 CSS 的 ellipsis 在更宽的标签页上表达。
 */
export function truncateToWidth(text: string, budgetUnits: number): string {
  let used = 0;
  let out = "";
  for (const ch of text) {
    const units = charWidthUnits(ch);
    if (used + units > budgetUnits) break;
    used += units;
    out += ch;
  }
  return out;
}

// markdown 噪音（模块级编译一次）。链接留文字、图片整个去掉；行首的标题 /
// 引用 / 列表符 / 任务框剥掉；行内的强调符与反引号剥掉。
const IMAGE_RE = /!\[[^\]]*\]\([^)]*\)/g;
const LINK_RE = /\[([^\]]*)\]\([^)]*\)/g;
const LEAD_RE = /^\s*(?:#{1,6}\s*|>\s?|(?:[-*+]|\d{1,9}[.)])\s+(?:\[[ xX]\]\s*)?)/;
const DECOR_RE = /\*\*|__|~~|[*_`]/g;

/**
 * 从正文派生标签页默认名。逐行找第一行「剥完噪音还剩东西」的文本；全都没有
 * （空正文 / 只有符号）返回 ""，调用方回落到纯数字名。
 */
export function autoTabName(text: string): string {
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine
      .replace(IMAGE_RE, " ")
      .replace(LINK_RE, "$1")
      .replace(LEAD_RE, "")
      .replace(DECOR_RE, "")
      .replace(/\s+/g, " ")
      .trim();
    if (line === "") continue;
    // 截断后可能以空格收尾（「code 开头」截到 5 单位 = "code "），再去一次尾空白
    return truncateToWidth(line, TAB_NAME_WIDTH_BUDGET).trim();
  }
  return "";
}
