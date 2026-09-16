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

// ---------------------------------------------------------------------------
// 反向：中文金额 → 数字
// ---------------------------------------------------------------------------

/**
 * 数字字表：**大写（壹贰…）、小写（一二…）、繁体变体（貳參陸兩）全收**。
 * 反向识别面对的是「别人递过来的一张纸条」，宽进严出：认字尽量宽，
 * 结构不对（乱字、缺单位、超范围）一律回 null，绝不猜。
 */
const REVERSE_DIGITS: Record<string, number> = {
  零: 0, 〇: 0,
  一: 1, 壹: 1,
  二: 2, 贰: 2, 貳: 2, 两: 2, 兩: 2,
  三: 3, 叁: 3, 参: 3, 參: 3,
  四: 4, 肆: 4,
  五: 5, 伍: 5,
  六: 6, 陆: 6, 陸: 6,
  七: 7, 柒: 7,
  八: 8, 捌: 8,
  九: 9, 玖: 9
};
const REVERSE_UNITS: Record<string, number> = {
  十: 10, 拾: 10,
  百: 100, 佰: 100,
  千: 1000, 仟: 1000
};
/** 与正向同一个量级上限：一万亿 */
const REVERSE_MAX_WHOLE = 1_000_000_000_000;

/**
 * 整数部分：亿 → 万 → 节内（仟佰拾）逐层累加。
 * 「拾伍」这种省掉「一」的口语写法（十位上没数字按 1 算）也认。
 */
function parseChineseInteger(text: string): number | null {
  let total = 0; // 已结算的亿级
  let section = 0; // 当前万级节内累计
  let number = 0; // 当前数字
  let seen = false;
  for (const ch of text) {
    if (ch === "零" || ch === "〇") {
      seen = true;
      number = 0;
      continue;
    }
    const digit = REVERSE_DIGITS[ch];
    if (digit !== undefined) {
      number = digit;
      seen = true;
      continue;
    }
    const unit = REVERSE_UNITS[ch];
    if (unit !== undefined) {
      section += (number === 0 ? 1 : number) * unit;
      number = 0;
      seen = true;
      continue;
    }
    if (ch === "万" || ch === "萬") {
      section = (section + number) * 10_000;
      number = 0;
      seen = true;
      continue;
    }
    if (ch === "亿" || ch === "億") {
      total = (total + section + number) * 100_000_000;
      section = 0;
      number = 0;
      seen = true;
      continue;
    }
    return null;
  }
  if (!seen) return null;
  const value = total + section + number;
  if (!Number.isSafeInteger(value) || value >= REVERSE_MAX_WHOLE) return null;
  return value;
}

/**
 * 中文金额转数字（元）。大写小写、简繁变体、`元/圆/圓`、`角`、`分`、结尾的
 * `整/正`、前缀 `负/負` 都认；「伍角」这种没有元的零头也认。
 * 认不出来（生造字、结构乱、超出一万亿）回 null，由调用方提示。
 */
export function fromChineseYuan(raw: string): number | null {
  let text = raw.trim().replace(/[\s,，、]/g, "");
  if (!text) return null;
  let negative = false;
  if (text.startsWith("负") || text.startsWith("負")) {
    negative = true;
    text = text.slice(1);
  }
  // 以「元/圆/圓」分界；没有元字但出现角/分/整的，整串按零头处理
  let intText = text;
  let fracText = "";
  const yuanAt = text.search(/[元圆圓]/);
  if (yuanAt >= 0) {
    intText = text.slice(0, yuanAt);
    fracText = text.slice(yuanAt + 1);
    // 「元整」这种整数部分整个缺失的不认（零元要写成「零元整」）
    if (!intText) return null;
  } else if (/[角分整正]/.test(text)) {
    intText = "";
    fracText = text;
  }
  if (!intText && !fracText) return null;
  const whole = intText ? parseChineseInteger(intText) : 0;
  if (whole === null) return null;
  // 角分部分：结尾的「整/正」是语气词，去掉；「零」只做占位
  const cleaned = fracText.replace(/[整正]+$/, "");
  let jiao = 0;
  let fen = 0;
  let pending: number | null = null;
  for (const ch of cleaned) {
    if (ch === "零" || ch === "〇") {
      pending = 0;
      continue;
    }
    const digit = REVERSE_DIGITS[ch];
    if (digit !== undefined) {
      pending = digit;
      continue;
    }
    if (ch === "角") {
      if (pending === null) return null;
      jiao = pending;
      pending = null;
      continue;
    }
    if (ch === "分") {
      if (pending === null) return null;
      fen = pending;
      pending = null;
      continue;
    }
    return null;
  }
  // 零头里剩下没落单位的数字（「伍元叁」）不认；「零」占位收尾可以
  if (pending !== null && pending !== 0) return null;
  const cents = whole * 100 + jiao * 10 + fen;
  const value = cents / 100;
  return negative ? -value : value;
}

/** 反向结果的展示格式：最多两位小数，尾部的 0 不啰嗦（1234.5 / 1234）。 */
export function formatYuanNumber(value: number): string {
  const fixed = Math.abs(value).toFixed(2);
  const trimmed = fixed.replace(/\.00$/, "").replace(/(\.\d)0$/, "$1");
  return (value < 0 ? "-" : "") + trimmed;
}
