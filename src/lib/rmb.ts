/**
 * 人民币金额大写（财务写法）：`1234.56` → `壹仟贰佰叁拾肆元伍角陆分`。
 *
 * 纯逻辑单独成模块，为的是能跑 node 环境的单元测试——这是一眼看不出来的规则堆
 * （零的合并、万亿分级、角分为零时的「整」），不能只靠手点两下确认。
 *
 * 复用 [`parseYuanToCents`] 解析输入：它已经与 core 的 `parse_cents` 同一口径
 * （去逗号、四舍五入到分、超安全整数即拒），金额换算不许有第二套实现。
 */
import { parseYuanToCents } from "./ledger";

const DIGITS = ["零", "壹", "贰", "叁", "肆", "伍", "陆", "柒", "捌", "玖"];
/** 四位一节的节内单位（0 → 空） */
const SECTION_UNITS = ["", "拾", "佰", "仟"];
/** 每四位的分级词（0 → 个级、1 → 万级、2 → 亿级） */
const GROUP_WORDS = ["", "万", "亿"];
/** 整数部分上限：一万亿（再大没有对应的分级词，不如直接说清楚） */
const MAX_INT_DIGITS = 12;

/** 四位一节（0~9999）转大写，节内自己处理「零」的合并。 */
function sectionToChinese(value: number): string {
  let out = "";
  let pendingZero = false;
  let unitIndex = 0;
  let rest = value;
  while (rest > 0) {
    const digit = rest % 10;
    if (digit === 0) {
      // 末尾的 0 不补「零」（壹仟零壹拾 后面那个 0 不该写成 零）
      if (out !== "") pendingZero = true;
    } else {
      out = DIGITS[digit] + SECTION_UNITS[unitIndex] + (pendingZero ? DIGITS[0] : "") + out;
      pendingZero = false;
    }
    rest = Math.floor(rest / 10);
    unitIndex += 1;
  }
  return out;
}

/**
 * 整数部分（十进制数字串）转大写。前导零会被忽略。
 *
 * **必须按四位一节走**：分级词（万 / 亿）是一节一个，挂在节内最高的那一位后面。
 * 早先按「每位一个分级词」写，`999999999999` 会写成「玖仟亿玖佰亿玖拾亿玖亿…」——
 * 位数一多就全乱。
 */
function integerToChinese(digits: string): string {
  const trimmed = digits.replace(/^0+/, "");
  if (trimmed === "") return DIGITS[0];
  const groups: number[] = [];
  for (let end = trimmed.length; end > 0; end -= 4) {
    groups.unshift(Number(trimmed.slice(Math.max(0, end - 4), end)));
  }
  let out = "";
  /** 前面有整节为零（只补一个「零」，且不在结尾补） */
  let pendingZero = false;
  for (let index = 0; index < groups.length; index++) {
    const value = groups[index];
    if (value === 0) {
      if (out !== "") pendingZero = true;
      continue;
    }
    // 本节高位是 0（如 `1亿零1` 里的 "0001"）也要补「零」，不然读成「壹亿壹元」
    const padded = groups[index].toString().padStart(4, "0");
    if (out !== "" && (pendingZero || padded.startsWith("0"))) out += DIGITS[0];
    pendingZero = false;
    out += sectionToChinese(value) + (GROUP_WORDS[groups.length - 1 - index] ?? "");
  }
  return out;
}

/**
 * 金额转大写。输入接受 `1234.56` / `1,234.56` / `-12` / `0.5` 这类写法
 * （与记账里输入金额同一套解析）。**解析不出来就回 null**，由调用方提示。
 */
export function toChineseYuan(raw: string): string | null {
  const cents = parseYuanToCents(raw);
  if (cents === null) return null;
  const negative = cents < 0;
  const absolute = Math.abs(cents);
  const whole = Math.floor(absolute / 100).toString();
  if (whole.length > MAX_INT_DIGITS) return null;
  const jiao = Math.floor((absolute % 100) / 10);
  const fen = absolute % 10;
  let out = (negative ? "负" : "") + integerToChinese(whole) + "元";
  if (jiao === 0 && fen === 0) return `${out}整`;
  if (jiao > 0) out += `${DIGITS[jiao]}角`;
  // 有分无角：中间要补一个「零」（壹元零伍分），不补就成了「壹元伍分」读不通
  else out += DIGITS[0];
  if (fen > 0) out += `${DIGITS[fen]}分`;
  return out;
}
