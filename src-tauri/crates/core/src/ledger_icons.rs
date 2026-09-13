//! 记账图标目录（与前端 `src/lib/ledgerIcons.ts` 的 `LEDGER_ICON_GROUPS` 一一对应）。
//!
//! 前端要的是「名字 → lucide 组件」，core 要的是「有哪些名字可以选」——CLI 的
//! `ledger icon-list` 把这份目录吐给 Agent/脚本，让 AI 能照着可用图标设计分类，
//! 而不是猜一个 lucide 里根本不存在的名字写进 `--icon`。
//!
//! **两边必须同步**：加图标 = 前端 import + `LEDGER_ICONS` + 恰好一个分组，然后重跑
//! `scripts` 里的一次性生成或手改这里；一致性由 `tests/ledger_icons.rs` 守着
//! （它 include_str! 前端那份 TS，逐个名字比对）。

/// 一个分组：中文组名 + 组内图标名（lucide 的 PascalCase 导出名）。
pub struct IconGroup {
    pub name: &'static str,
    pub icons: &'static [&'static str],
}

pub static ICON_GROUPS: &[IconGroup] = &[
    IconGroup {
        name: "饮食",
        icons: &[
            "Utensils",
            "UtensilsCrossed",
            "Coffee",
            "CupSoda",
            "Candy",
            "Apple",
            "Carrot",
            "Cake",
            "IceCreamCone",
            "Wine",
            "Beer",
            "Soup",
            "Sandwich",
            "Pizza",
            "Salad",
            "Fish",
        ],
    },
    IconGroup {
        name: "娱乐",
        icons: &[
            "Gamepad2",
            "Clapperboard",
            "Music",
            "Headphones",
            "Tv",
            "Film",
            "Palette",
            "Camera",
            "Guitar",
            "Piano",
            "PartyPopper",
            "Mic",
        ],
    },
    IconGroup {
        name: "购物",
        icons: &[
            "ShoppingBag",
            "ShoppingCart",
            "Store",
            "ShoppingBasket",
            "Tag",
            "Tags",
            "Receipt",
            "PackageOpen",
            "Truck",
            "BadgePercent",
            "Barcode",
            "Gift",
        ],
    },
    IconGroup {
        name: "交通",
        icons: &[
            "Bus",
            "Car",
            "CarFront",
            "CarTaxiFront",
            "TrainFront",
            "TramFront",
            "Plane",
            "Bike",
            "Fuel",
            "SquareParking",
            "Ship",
            "Motorbike",
        ],
    },
    IconGroup {
        name: "旅行",
        icons: &[
            "Luggage",
            "Map",
            "MapPin",
            "Compass",
            "Globe",
            "Ticket",
            "Tickets",
            "Building2",
            "PlaneTakeoff",
            "Sunrise",
            "Umbrella",
        ],
    },
    IconGroup {
        name: "居住家具",
        icons: &[
            "House",
            "KeyRound",
            "Wrench",
            "Zap",
            "Flame",
            "DoorOpen",
            "BedDouble",
            "Sofa",
            "Armchair",
            "Lamp",
            "Bath",
            "Lightbulb",
        ],
    },
    IconGroup {
        name: "家庭生活",
        icons: &[
            "HeartHandshake",
            "Baby",
            "Users",
            "UserRound",
            "CookingPot",
            "Flower2",
            "Leaf",
            "Sprout",
            "Recycle",
            "HandHeart",
            "Home",
        ],
    },
    IconGroup {
        name: "医疗健康",
        icons: &[
            "HeartPulse",
            "Pill",
            "Stethoscope",
            "Syringe",
            "Thermometer",
            "Activity",
            "Brain",
            "Eye",
            "Ear",
            "Hospital",
            "Bandage",
            "ShieldPlus",
        ],
    },
    IconGroup {
        name: "学习教育",
        icons: &[
            "BookOpen",
            "PenLine",
            "GraduationCap",
            "BookMarked",
            "Library",
            "NotebookPen",
            "Pencil",
            "Ruler",
            "Calculator",
            "School",
            "Award",
        ],
    },
    IconGroup {
        name: "办公工作",
        icons: &[
            "Briefcase",
            "FileText",
            "Printer",
            "ClipboardList",
            "FolderOpen",
            "Paperclip",
            "Monitor",
            "Laptop",
            "Presentation",
            "CalendarDays",
            "Clock",
            "Megaphone",
        ],
    },
    IconGroup {
        name: "通讯网络",
        icons: &[
            "MessageCircle",
            "Wifi",
            "Phone",
            "PhoneCall",
            "Mail",
            "Send",
            "AtSign",
            "Network",
            "Router",
            "Signal",
            "Bluetooth",
            "Share2",
        ],
    },
    IconGroup {
        name: "金融理财",
        icons: &[
            "Wallet",
            "Landmark",
            "CreditCard",
            "PiggyBank",
            "Coins",
            "TrendingUp",
            "ChartLine",
            "Percent",
            "Scale",
            "Bitcoin",
            "WalletCards",
            "Vault",
            "ReceiptText",
        ],
    },
    IconGroup {
        name: "收入",
        icons: &[
            "BadgeDollarSign",
            "HandCoins",
            "Wallet2",
            "Medal",
            "Banknote",
            "BanknoteArrowUp",
            "RotateCcw",
            "CircleDollarSign",
            "Handshake",
            "Currency",
        ],
    },
    IconGroup {
        name: "运动健身",
        icons: &[
            "Dumbbell",
            "PersonStanding",
            "Volleyball",
            "Trophy",
            "SportShoe",
            "Target",
            "Timer",
            "HandFist",
        ],
    },
    IconGroup {
        name: "个人护理",
        icons: &[
            "Scissors",
            "Droplet",
            "Glasses",
            "Sun",
            "Moon",
            "Flower",
            "Heart",
            "HandMetal",
            "Sparkle",
            "MirrorRound",
        ],
    },
    IconGroup {
        name: "服饰美容",
        icons: &[
            "Shirt",
            "Sparkles",
            "Brush",
            "Crown",
            "Footprints",
            "Watch",
            "Ribbon",
            "Handbag",
            "Gem",
            "HatGlasses",
        ],
    },
    IconGroup {
        name: "宠物",
        icons: &[
            "Dog",
            "Bone",
            "PawPrint",
            "Cat",
            "Bird",
            "Rabbit",
            "Squirrel",
            "Turtle",
            "Bug",
            "Worm",
        ],
    },
    IconGroup {
        name: "数码",
        icons: &[
            "Smartphone",
            "Tablet",
            "Mouse",
            "Keyboard",
            "HardDrive",
            "Cpu",
            "MemoryStick",
            "Usb",
            "Speaker",
            "Plug",
        ],
    },
    IconGroup {
        name: "运动户外",
        icons: &[
            "Tent",
            "Mountain",
            "FlameKindling",
            "FishingRod",
            "Backpack",
            "TreePine",
            "Trees",
            "Waves",
            "Sailboat",
            "Wind",
        ],
    },
    IconGroup {
        name: "通用",
        icons: &[
            "Package",
            "Ellipsis",
            "CircleDot",
            "Star",
            "Check",
            "Info",
            "CircleAlert",
            "CircleHelp",
            "Settings",
            "Sliders",
            "Filter",
            "Flag",
            "Bell",
            "ArrowLeftRight",
        ],
    },
];

/// 全部图标名（去重后的扁平列表，顺序 = 分组顺序）。
pub fn all_icons() -> Vec<&'static str> {
    let mut out = Vec::with_capacity(228);
    for group in ICON_GROUPS {
        for icon in group.icons {
            if !out.contains(icon) {
                out.push(icon);
            }
        }
    }
    out
}

/// 这个名字在目录里吗（给 `--icon` 做校验用；不在目录里的名字前端画不出来）。
pub fn is_known_icon(name: &str) -> bool {
    ICON_GROUPS.iter().any(|group| group.icons.contains(&name))
}
