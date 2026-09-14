//! 账户类型预置表（v0.7.4）：类型是自由字符串，这里只是"新建账户时给你一排候选"。
//! 老的五种 kind（cash/debit/credit/investment/other）沿用 ledgerIcons 里的默认
//! 图标/标签/颜色，新预置补上电子支付/社保/卡券/投资理财/借贷/虚拟货币这些语义。
//! v0.7.5：用户自定义的类型持久化在账本里（book.accountTypes，账户的 kind 字符串
//! = 类型名），helper 的第二个参数把这张表带进来——预置查不到就查自定义，
//! 再查不到标签回落到 kind 字符串本身、颜色回落到灰——界面上不开天窗。

import { ACCOUNT_KIND_COLOR, ACCOUNT_KIND_ICON, ACCOUNT_KIND_LABEL } from "./ledgerIcons";
import type { LedgerAccountKind, LedgerAccountType } from "./types";

export type AccountTypePreset = {
  kind: LedgerAccountKind;
  label: string;
  icon: string;
  color: string;
};

export const ACCOUNT_TYPE_PRESETS: AccountTypePreset[] = [
  { kind: "cash", label: "现金", icon: "Wallet", color: "#e8a33d" },
  { kind: "bank", label: "银行卡", icon: "WalletCards", color: "#3d8bfd" },
  { kind: "debit", label: "储蓄卡", icon: "Landmark", color: "#b23a48" },
  { kind: "credit", label: "信用卡", icon: "CreditCard", color: "#c0392b" },
  { kind: "wechat", label: "微信", icon: "MessageCircle", color: "#2aae67" },
  { kind: "alipay", label: "支付宝", icon: "Smartphone", color: "#1677ff" },
  { kind: "drmb", label: "数字人民币", icon: "Currency", color: "#e0654f" },
  { kind: "housing", label: "公积金", icon: "House", color: "#7cb342" },
  { kind: "medical", label: "医保", icon: "HeartPulse", color: "#d94f70" },
  { kind: "transit", label: "公交卡", icon: "Bus", color: "#f0862c" },
  { kind: "meal", label: "饭卡", icon: "Utensils", color: "#e8a33d" },
  { kind: "deposit", label: "押金", icon: "KeyRound", color: "#9b59b6" },
  { kind: "stock", label: "股票", icon: "ChartLine", color: "#6b7fd7" },
  { kind: "funds", label: "基金", icon: "TrendingUp", color: "#16a5a5" },
  { kind: "borrow", label: "借入", icon: "HandCoins", color: "#c0392b" },
  { kind: "lend", label: "借出", icon: "Handshake", color: "#7cb342" },
  { kind: "crypto", label: "虚拟货币", icon: "Bitcoin", color: "#f7931a" },
  { kind: "investment", label: "投资", icon: "PiggyBank", color: "#2980b9" },
  { kind: "other", label: "其他", icon: "Ellipsis", color: "#7f8c8d" }
];

function preset(kind: string): AccountTypePreset | undefined {
  return ACCOUNT_TYPE_PRESETS.find((item) => item.kind === kind);
}

/** 自定义类型的账户 kind = 类型名（自由字符串），按名字回查它的图标与颜色 */
function custom(kind: string, customs?: LedgerAccountType[]): LedgerAccountType | undefined {
  return customs?.find((item) => item.name === kind);
}

export function accountTypeLabel(kind: string): string {
  return preset(kind)?.label || ACCOUNT_KIND_LABEL[kind] || kind;
}

export function accountTypeIcon(kind: string, customs?: LedgerAccountType[]): string {
  return preset(kind)?.icon || custom(kind, customs)?.icon || ACCOUNT_KIND_ICON[kind] || "Wallet";
}

export function accountTypeColor(kind: string, customs?: LedgerAccountType[]): string {
  return preset(kind)?.color || custom(kind, customs)?.color || ACCOUNT_KIND_COLOR[kind] || "#7f8c8d";
}
