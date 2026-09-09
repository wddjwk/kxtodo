//! 记账图标白名单：分类/账户存的是 lucide 图标名字符串（同步与 Excel 都只带名字），
//! 界面在这里把名字换回组件。名字不在白名单里就回退到按类型/按侧的默认图标——
//! 老数据或手填的怪名字不该让界面开天窗。

import {
  Apple,
  Banknote,
  Bike,
  Bone,
  BookOpen,
  Briefcase,
  Bus,
  Candy,
  Carrot,
  CarTaxiFront,
  ChartLine,
  CircleDot,
  Clapperboard,
  Coffee,
  Coins,
  CreditCard,
  CupSoda,
  Dog,
  Dumbbell,
  Ellipsis,
  FileText,
  Flame,
  Fuel,
  Gamepad2,
  Gift,
  HeartHandshake,
  HeartPulse,
  House,
  KeyRound,
  Landmark,
  Medal,
  MessageCircle,
  Music,
  Package,
  PartyPopper,
  PawPrint,
  PenLine,
  Percent,
  PiggyBank,
  Pill,
  Plane,
  ReceiptText,
  RotateCcw,
  Shirt,
  ShoppingBag,
  ShoppingCart,
  Smartphone,
  Sparkles,
  SquareParking,
  Stethoscope,
  TrainFront,
  TrendingUp,
  Utensils,
  UtensilsCrossed,
  Wallet,
  WalletCards,
  Wifi,
  Wrench,
  Zap,
  ArrowLeftRight,
  type LucideIcon
} from "@lucide/svelte";
import type { LedgerAccountKind, LedgerSide } from "./types";

export const LEDGER_ICONS: Record<string, LucideIcon> = {
  Apple,
  Banknote,
  Bike,
  Bone,
  BookOpen,
  Briefcase,
  Bus,
  Candy,
  Carrot,
  CarTaxiFront,
  ChartLine,
  CircleDot,
  Clapperboard,
  Coffee,
  Coins,
  CreditCard,
  CupSoda,
  Dog,
  Dumbbell,
  Ellipsis,
  FileText,
  Flame,
  Fuel,
  Gamepad2,
  Gift,
  HeartHandshake,
  HeartPulse,
  House,
  KeyRound,
  Landmark,
  Medal,
  MessageCircle,
  Music,
  Package,
  PartyPopper,
  PawPrint,
  PenLine,
  Percent,
  PiggyBank,
  Pill,
  Plane,
  ReceiptText,
  RotateCcw,
  Shirt,
  ShoppingBag,
  ShoppingCart,
  Smartphone,
  Sparkles,
  SquareParking,
  Stethoscope,
  TrainFront,
  TrendingUp,
  Utensils,
  UtensilsCrossed,
  Wallet,
  WalletCards,
  Wifi,
  Wrench,
  Zap,
  ArrowLeftRight
};

/** 分类/账户管理器的图标选择网格：覆盖日常收支场景的常用图标。 */
export const LEDGER_ICON_CHOICES: string[] = [
  "Utensils",
  "Coffee",
  "Candy",
  "CupSoda",
  "Apple",
  "Carrot",
  "Bus",
  "TrainFront",
  "CarTaxiFront",
  "Plane",
  "Fuel",
  "SquareParking",
  "Bike",
  "House",
  "KeyRound",
  "Zap",
  "Flame",
  "Wifi",
  "Wrench",
  "ShoppingBag",
  "ShoppingCart",
  "Shirt",
  "Smartphone",
  "Sparkles",
  "Gamepad2",
  "Clapperboard",
  "Music",
  "Dumbbell",
  "HeartPulse",
  "Pill",
  "Stethoscope",
  "BookOpen",
  "PenLine",
  "Gift",
  "PartyPopper",
  "HeartHandshake",
  "Dog",
  "Bone",
  "PawPrint",
  "Banknote",
  "Medal",
  "Coins",
  "TrendingUp",
  "Percent",
  "ChartLine",
  "Briefcase",
  "FileText",
  "RotateCcw",
  "ReceiptText",
  "Wallet",
  "WalletCards",
  "CreditCard",
  "PiggyBank",
  "Landmark",
  "MessageCircle",
  "ArrowLeftRight",
  "Package",
  "CircleDot",
  "Ellipsis"
];

export const ACCOUNT_KIND_ICON: Record<LedgerAccountKind, string> = {
  cash: "Wallet",
  debit: "Landmark",
  credit: "CreditCard",
  investment: "PiggyBank",
  other: "WalletCards"
};

export const ACCOUNT_KIND_LABEL: Record<LedgerAccountKind, string> = {
  cash: "现金",
  debit: "储蓄卡",
  credit: "信用卡",
  investment: "投资",
  other: "其他"
};

export const ACCOUNT_KIND_COLOR: Record<LedgerAccountKind, string> = {
  cash: "#e8a33d",
  debit: "#b23a48",
  credit: "#c0392b",
  investment: "#2980b9",
  other: "#7f8c8d"
};

export function ledgerIcon(name: string | undefined, fallback: string): LucideIcon {
  if (name && LEDGER_ICONS[name]) return LEDGER_ICONS[name];
  return LEDGER_ICONS[fallback] ?? Ellipsis;
}

export function accountIconName(icon: string, kind: LedgerAccountKind): string {
  return icon && LEDGER_ICONS[icon] ? icon : ACCOUNT_KIND_ICON[kind];
}

/** 收支两侧的默认强调色：支出红、收入绿（热力图与统计图同口径）。 */
export const SIDE_COLOR: Record<LedgerSide, string> = {
  expense: "#d9534f",
  income: "#2f9e6e"
};

export const SIDE_LABEL: Record<LedgerSide, string> = {
  expense: "支出",
  income: "收入"
};

export const TRANSFER_ICON = "ArrowLeftRight";
