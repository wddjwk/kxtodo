import type { NavItemId, NavLayout } from "./nav";

export type NodeKind = "system" | "category" | "entry";

/** 条目渲染类型：todo = 可勾选的待办卡片（默认）；card = 一般卡片（隐藏勾选框，展示型） */
export type CardStyle = "todo" | "card";

export type AppNode = {
  id: string;
  kind: NodeKind;
  name: string;
  icon: string;
  parentId: string | null;
  order?: number;
  collapsed?: boolean;
  cardStyle?: CardStyle;
  createdAt: string;
  /** core 的 Node 有这一字段（同步 LWW 与「最近改过」排序都读它）；
   *  normalize 漏掉就等于每次快照刷新都把它抹掉（`dueTime` 的同类，v0.8.3 补）。 */
  updatedAt?: string;
};

/// 七彩虹 + 粉 + 灰 + 自定义（`custom` 必须配 `hex`，否则按 gray 渲染）
export type TagColor =
  | "red"
  | "orange"
  | "yellow"
  | "green"
  | "cyan"
  | "blue"
  | "purple"
  | "pink"
  | "gray"
  | "custom";

export type Tag = {
  id: string;
  color: TagColor;
  text?: string;
  /** 自定义配色的 `#rrggbb`（只在 color === "custom" 时有意义） */
  hex?: string;
};

/**
 * 任务提醒规则（与 core `model::Reminder` 同构，`kind` 是判别式）。
 *
 * - `absolute`：绝对时刻，`at` 是 RFC3339（core 落盘时规范化成 UTC 毫秒）
 * - `beforeDue`：截止前 N 分钟（0 = 到点时），跟着 dueDate/dueTime 走
 *
 * 「哪一条已经响过」不进这个类型——那是本机 runtime 的台账，不参与同步。
 */
export type ReminderRule =
  | { kind: "absolute"; at: string }
  | { kind: "beforeDue"; minutes: number };

export type Task = {
  id: string;
  nodeId: string;
  order?: number;
  markdown: string;
  completed: boolean;
  important: boolean;
  pinned?: boolean;
  myDay: boolean;
  plannedDate?: string;
  dueDate?: string;
  /** 到期时刻 HH:MM；空/缺省 = 只精确到天 */
  dueTime?: string;
  /** 提醒规则；空/缺省 = 没有提醒 */
  reminders?: ReminderRule[];
  completedAt?: string;
  tags: Tag[];
  emojis: string[];
  expanded?: boolean;
  createdAt: string;
  updatedAt?: string;
};

export type EmojiPickerTarget = {
  taskId: string;
  index: number;
};

/** 日记视图：列表（时间轴）/ 日历（月历 + 当日卡片）/ 分组（按年月折叠） */
export type DiaryViewMode = "list" | "calendar" | "group";

export type DiaryEntry = {
  id: string;
  /** 归属日期 YYYY-MM-DD（可以后补写别的日子，不等于 createdAt 的日期） */
  date: string;
  /** 标题；空 = 卡片直接展示正文首行 */
  title: string;
  markdown: string;
  /** 心情 emoji；空 = 没记 */
  mood: string;
  /** 天气 emoji；空 = 没记 */
  weather: string;
  tags: Tag[];
  /** 本机 UI 状态，不参与同步 */
  expanded?: boolean;
  createdAt: string;
  updatedAt?: string;
};

/** 日记编辑器的目标：改已有的一篇（id），或新建一篇归到某天（date）。 */
export type DiaryEditorTarget = { id: string } | { date: string };

/** 日程视图（v0.9.0）：月 / 周 / 三日 / 日，默认周视图。 */
export type CalendarViewMode = "month" | "week" | "threeday" | "day";

/**
 * 日程事件（calendar.json 的前端形态，与 core `model::CalendarEvent` 同构）。
 * 时间是本地墙上时间：startDate/endDate 是 YYYY-MM-DD，startTime/endTime 是 HH:MM
 * （全天事件为空）。`taskId` 非空 = 从 todo 条目「添加到日程」创建的关联事件，
 * 详情与条目内容双向编辑。
 */
export type CalendarEvent = {
  id: string;
  title: string;
  /** 详细信息（Markdown） */
  details: string;
  startDate: string;
  startTime: string;
  endDate: string;
  endTime: string;
  allDay: boolean;
  /** 提醒规则（与任务共用一套）；beforeDue = 开始前 N 分钟（全天按 09:00 折算） */
  reminders: ReminderRule[];
  /** 地点（v0.9.1，自由文本） */
  location: string;
  /** 分类 id（v0.9.1）；空 = 无分类，各视图用日程主题色 */
  categoryId: string;
  /** 重复规则（v0.9.1）；null = 不重复 */
  repeat: CalendarRepeat | null;
  /** 重复系列的例外日期（「只删这一次」写在这里） */
  exceptDates: string[];
  /** 关联的 todo 条目 id；空 = 独立日程 */
  taskId: string;
  createdAt: string;
  updatedAt?: string;
};

/**
 * 重复频率与结束条件（v0.9.1）。语义与 core `model::CalendarRepeat` 同源：
 * 周频率以开始日期所在周的**周一**为锚；月/年频率遇到不存在的日号跳过该次；
 * `count` 数的是规则生成的次数（含首次），例外日期同样占名额。
 */
export type RepeatFreq = "day" | "week" | "month" | "year";
export type RepeatEnd = "never" | "count" | "until";

export type CalendarRepeat = {
  freq: RepeatFreq;
  /** 每 N 个单位一次（≥1） */
  interval: number;
  /** freq = week：0 = 周日 … 6 = 周六；空 = 只按开始日期的星期 */
  byWeekday: number[];
  /** freq = month/year：1..31；空 = 按开始日期的日号 */
  byMonthDay: number[];
  end: RepeatEnd;
  /** end = count 时的总次数（含首次） */
  count: number;
  /** end = until 时最后一次出现的日期（闭区间） */
  until: string;
};

/** 日程分类（v0.9.1）：名字 + 颜色，颜色就是各视图的展示色。 */
export type CalendarCategory = {
  id: string;
  name: string;
  /** #rrggbb；空 = 用日程主题色 */
  color: string;
  /** 新建日程时默认选中它（全局最多一个） */
  isDefault: boolean;
  order: number;
  createdAt: string;
  updatedAt?: string;
};

/**
 * 日程编辑器的目标：改已有的一条（id），或新建（预置起止/全天/关联任务）。
 * 新建**保存时才落盘**——拖选时间后改主意直接关掉，不留空日程。
 */
export type CalendarEditorTarget =
  | { id: string }
  | {
      startDate: string;
      startTime?: string;
      endDate?: string;
      endTime?: string;
      allDay?: boolean;
      taskId?: string;
    };

/** 记账条目类型：转账不计入收支统计，只改两个账户的余额。 */
export type LedgerKind = "expense" | "income" | "transfer";
/** 分类归属侧：支出与收入各有一套分类。 */
export type LedgerSide = "expense" | "income";
/** 账户类型：预置几种（cash/debit/credit/investment/other），v0.7.4 起允许自定义字符串 */
export type LedgerAccountKind = string;

export type LedgerAccount = {
  id: string;
  name: string;
  /** lucide 图标名（ledgerIcons 白名单）；空 = 按类型取默认 */
  icon: string;
  color: string;
  kind: LedgerAccountKind;
  /** 期初余额（分）；当前余额 = 期初 + 流水推导 */
  initialCents: number;
  note: string;
  order: number;
  createdAt: string;
  updatedAt?: string;
};

export type LedgerCategory = {
  id: string;
  name: string;
  side: LedgerSide;
  /** 大类 id；空 = 自己就是大类 */
  parentId?: string;
  icon: string;
  /** 空 = 继承大类颜色 */
  color: string;
  order: number;
  createdAt: string;
  updatedAt?: string;
};

export type LedgerEntry = {
  id: string;
  kind: LedgerKind;
  /** 金额（分），恒为正；方向由 kind 决定 */
  amountCents: number;
  /** 支出/转账 = 付款账户；收入 = 收款账户 */
  accountId: string;
  /** 转账的转入账户 */
  toAccountId?: string;
  categoryId?: string;
  /** 归属日期 YYYY-MM-DD */
  date: string;
  /** HH:MM:SS；空 = 只记到天 */
  time: string;
  note: string;
  /** 这条账的插图文件名列表（img/data/ledger/ 下）；缺省/空 = 没有 */
  images?: string[];
  createdAt: string;
  updatedAt?: string;
};

/** 记账视图：列表（按天卡片）/ 日历（热力图）/ 统计（曲线+占比）/ 资产（账户） */
export type LedgerViewMode = "list" | "calendar" | "stats" | "assets";

/** 记账面板的目标：改已有的一笔（id），或新建一笔归到某天某种类型（date+kind）。 */
export type LedgerEditorTarget =
  | { id: string }
  | { date: string; kind: LedgerKind };

/** 用户自定义的账户类型（预置表之外的）：带图标与颜色，可编辑/删除，随账本同步。
 *  账户自己的 kind 仍是自由字符串——删掉类型不影响已建的账户。 */
export type LedgerAccountType = {
  id: string;
  name: string;
  /** lucide 图标名（账户图标目录）；空 = Wallet */
  icon: string;
  color: string;
  createdAt?: string;
  updatedAt?: string;
};

/** 整本账：账户 / 分类 / 流水 / 自定义账户类型（ledger.json 的前端形态）。 */
export type LedgerBook = {
  accounts: LedgerAccount[];
  categories: LedgerCategory[];
  entries: LedgerEntry[];
  accountTypes: LedgerAccountType[];
};

/**
 * 全局搜索的一条命中：任务卡片（todo / 一般卡片按所属条目的 cardStyle）、日记卡片
 * 或一条账。结果界面按这个 union 混排，三种卡片各走各的组件。
 */
export type SearchHit =
  | { kind: "task"; key: string; task: Task; cardStyle: CardStyle }
  | { kind: "diary"; key: string; entry: DiaryEntry }
  | { kind: "ledger"; key: string; entry: LedgerEntry };

export type ProfileSettings = {
  displayName: string;
  email: string;
  avatar: string;
};

export type ThemePreset = {
  name: string;
  color: string;
};

export type SyncMode = "lan" | "server" | "p2p";

export type Settings = {
  profile: ProfileSettings;
  appearance: {
    linkOpenMode: "app" | "system";
    listSortMode: import("./sort").SortMode;
    uiScale: number;
    /** UI 字号：分组分类与页面标题（我的一天那些）吃它 */
    uiFontSize: number;
    /** 正文字号：任务卡/日记的 markdown 正文 */
    markdownFontSize: number;
    /** 记账页字号：那一页信息密度高，独立于 UI 字号 */
    ledgerFontSize: number;
    /** 日记页字号 */
    diaryFontSize: number;
    editorFontSize: number;
    editorWidthPercent: number;
    editorHeightPercent: number;
    tagFontSize: number;
    themePresets: ThemePreset[];
    /** 预置标签：右键菜单「标签」面板里可一键添加的常用标签（跟着设置同步） */
    tagPresets: Tag[];
    /** 日记专用预置标签（v0.8.3）：与 tagPresets 分开两套、分开展示 */
    diaryTagPresets: Tag[];
    uiColors: Record<string, string>;
    /** 固定导航里显示哪些行、按什么顺序（id 见 nav.ts） */
    navItems: NavItemId[];
    /** 固定导航的展示方式：单列 / 双列 / 只图标 */
    navLayout: NavLayout;
    /** 临期高亮配色：按节点 id 存三个色（今天/明天/后天）。
     *  **每个页面一套**，不全局统一；没有条目的页面用默认三色。 */
    dueColors: Record<string, string[]>;
    /** 新建分组/条目的默认外观；空串 = 不配置，跟随应用默认 */
    newNodeDefaults: {
      accent: string;
      backgroundColor: string;
      backgroundImage: string;
      backgroundOpacity: number;
    };
  };
  lifecycle: {
    closeToTray: boolean;
    launchAtStartup: boolean;
  };
  notifications: {
    durationMs: number;
    position: NotificationPosition;
    width: number;
    height: number;
    titleFontSize: number;
    bodyFontSize: number;
  };
  shortcuts: {
    newTask: string;
    focusSearch: string;
    toggleWindow: string;
    openSettings: string;
    syncNow: string;
  };
  sync: {
    enabled: boolean;
    /** 通信方式：局域网 / 自建服务 / P2P（后续版本）。三种方式共用同一套同步内核 */
    mode: SyncMode;
    /** 自建服务方式的服务器地址 */
    serverUrl: string;
    /** 局域网：本机作为服务器（内置 server 随应用启停；与 lanPeer 二选一） */
    lanHost: boolean;
    /** 局域网：本机作为服务器时的监听端口（被占用会自动向上找） */
    lanPort: number;
    /** 局域网：本机作为服务器时的展示名 = 它在局域网内的身份（要求唯一） */
    lanName: string;
    /** 局域网：选定的远端主机名（从发现列表里点选） */
    lanPeer: string;
    /** P2P 高级覆盖：自部署 iroh relay 地址（空 = n0 免费公共服务；disabled = 不用 relay） */
    p2pRelay: string;
    /** P2P 高级覆盖：自部署 pkarr 目录地址（空 = n0 免费公共服务） */
    p2pDirectory: string;
    username: string;
    secret: string;
    syncData: boolean;
    syncSettings: boolean;
    syncSchedules: boolean;
    syncDiary: boolean;
    syncLedger: boolean;
    syncCalendar: boolean;
    intervalSeconds: number;
    reconnectSeconds: number;
  };
  /** 文件传输助手（v0.8.3）：本机设置，不参与同步 */
  transfer: {
    /** 自选 iroh relay（空 = 跟 p2p 同步同一个；disabled = 不用 relay） */
    relay: string;
    /** 本机在传输房间里展示的设备名（v0.8.4；本机偏好，不跨设备同步） */
    deviceName: string;
    /** 自动接收：开着就不再弹接收确认卡（默认关） */
    autoAccept: boolean;
  };
  /** 剪切板同步（v0.8.10）：本机设置，不参与同步 */
  clipboard: {
    enabled: boolean;
    /** 设备名自定义前缀；空 = kxtodo。显示名统一追加 ·平台 */
    deviceName: string;
    lanEnabled: boolean;
    /** 广域网：统一服务器发现 */
    serverEnabled: boolean;
    /** 广域网：P2P 账户房间发现 */
    wanP2pEnabled: boolean;
    /** 统一服务器地址；可省略 http:// 与端口 */
    serverUrl: string;
    /** 广域网共用账户名 */
    account: string;
    /** 广域网共用密码（只存本机） */
    password: string;
    /** HTML 不再是独立一档（v0.9.0 bugfix 4.1）：剪切板里的 html 折成文件走 files */
    types: { text: boolean; image: boolean; files: boolean };
    limits: { textMb: number; imageMb: number; filesMb: number };
    /** 收到文件落盘目录；空 = 宿主下载目录下的 kxtodo-clipboard */
    saveDir: string;
  };
  syncUpdatedAt?: string;
  updates: {
    autoCheck: boolean;
  };
  features: {
    showCategoryBadges: boolean;
    pinnedIcon: boolean;
    pinnedSection: boolean;
    sync: boolean;
    editorToolbar: boolean;
    /** 移动端页面左上角的返回箭头（默认关，桌面无感） */
    mobileBack: boolean;
    /** 超链接渲染样式（三档单选，默认 card）：off = 原样链接；
     *  title = 裸链接抓网页标题按 [标题](链接) 渲染；card = 所有超链接渲染成预览卡
     *  （抓不到网页信息就退回原样链接）。**不可多选**——「都选」没有额外含义。 */
    linkRender: "off" | "title" | "card";
    /** 一周的第一天（日历视图 / 日期选择器 / 周统计 /「本周」分组共用） */
    weekStart: "monday" | "sunday";
    /** 临期高亮：off 不画；solid 按档取整色；gradient 按剩余时间在三个锚点间插值 */
    dueHighlight: "off" | "solid" | "gradient";
    /** 安卓后台常驻（v0.9.0，本机设置不同步）：前台服务保活，同步与提醒在后台继续跑 */
    androidKeepAlive: boolean;
    /** 常驻通知的标语文本（默认「今天有什么要做的吗~」） */
    keepAliveNotice: string;
  };
  /** 日记偏好。view 是本机状态；主题色与背景跟着设置同步走（外观该多端一致）。 */
  diary: {
    view: DiaryViewMode;
    /** 主题色 #rrggbb；空 = 用默认日记色 */
    accent: string;
    backgroundColor: string;
    /** `img:<文件名>` 或 http(s)/data URL；空 = 无图 */
    backgroundImage: string;
    backgroundOpacity: number;
  };
  /** 记账偏好。与日记同一条口径：view 本机状态，外观跟着设置同步走。 */
  ledger: {
    view: LedgerViewMode;
    accent: string;
    backgroundColor: string;
    backgroundImage: string;
    backgroundOpacity: number;
  };
  /** 日程偏好（v0.9.0）。与日记/记账同一条口径：view 本机状态，外观跟着设置同步走。 */
  calendar: {
    view: CalendarViewMode;
    /** 时间网格缩放倍率（v0.9.1）：1 = 一屏刚好 12 小时；本机偏好，不进同步 */
    hourZoom: number;
    accent: string;
    backgroundColor: string;
    backgroundImage: string;
    backgroundOpacity: number;
  };
  /** 工具箱外观（v0.8.4）。工具页三点菜单就两件事，字段也只有这两个。 */
  toolbox: {
    /** 主题色 #rrggbb；空 = 用默认主题色（工具箱主界面） */
    accent: string;
    backgroundColor: string;
    /** 每个工具子页自己的主题色（工具 id → #rrggbb）；缺省跟主界面（v0.8.7） */
    toolAccents: Record<string, string>;
    /** 每个工具子页自己的背景色（工具 id → #rrggbb）；缺省跟主界面 */
    toolBackgrounds: Record<string, string>;
  };
};

export type ListBackground = {
  color: string;
  image?: string;
  imageOpacity?: number;
};

export type SchedulerRuntimeKey = "python" | "node" | "pwsh" | "bash" | "make";

/**
 * 草稿纸的一个标签页（v0.9.0；v0.9.1 起默认名从内容派生）：
 * nameAuto=true = 名字仍是机器给的默认名，随正文内容重派生；
 * false = 用户手动改过名，内容再变也不覆盖。
 */
export type ScratchpadTab = {
  id: string;
  name: string;
  nameAuto: boolean;
  text: string;
  updatedAt?: string;
};

export type SchedulerRuntimePaths = Record<SchedulerRuntimeKey, string>;

export type SchedulerCondition = {
  enabled: boolean;
  mode: "contains" | "regex";
  pattern: string;
  stream?: "stdout" | "stderr";
};

export type SchedulerScriptLanguage = "python" | "javascript" | "powershell" | "bash" | "makefile" | "custom";

export type NotificationTone = "info" | "success" | "warning" | "error";
export type NotificationPosition = "bottom-right" | "top-right" | "bottom-left" | "top-left";

export type AppNotification = {
  title: string;
  message: string;
  durationMs: number;
  tone: NotificationTone;
  position?: NotificationPosition;
};

export type SchedulerStdoutNotification = {
  enabled: boolean;
  condition: SchedulerCondition;
  notification: AppNotification;
};

export type ScheduledTaskAction = {
  type: "script" | "executable" | "notification";
  scriptMode: "path" | "inline";
  language: SchedulerScriptLanguage;
  interpreter: string;
  filePath: string;
  code: string;
  executablePath: string;
  arguments: string;
  workingDirectory: string;
  timeout?: string;
  notification: AppNotification;
  notifyOnComplete: boolean;
  completionNotification: AppNotification;
  stdoutNotification: SchedulerStdoutNotification;
};

export type ScheduledTaskTrigger = {
  type: "once" | "interval" | "calendar" | "condition";
  runAt: string;
  everySeconds: number;
  repeatCount: number;
  cron: string;
  timezone?: string;
  missedPolicy?: "skip" | "run-once";
  cooldown?: string;
  stopCondition: SchedulerCondition;
  probeAction: ScheduledTaskAction;
  probeCondition: SchedulerCondition;
};

export type SchedulerGate = {
  windows: Array<{ start: string; end: string; weekdays?: number[] }>;
  probeAction?: ScheduledTaskAction;
  condition?: SchedulerCondition;
};

export type ScheduleHistoryRun = {
  taskId: string;
  kind: string;
  startedAt: string;
  finishedAt: string;
  status: string;
  exitCode?: number | null;
  stdout?: string;
  stderr?: string;
  stopReason?: string | null;
};

export type ScheduledTaskStatus = "idle" | "running" | "success" | "failed" | "stopped";

export type ScheduledTask = {
  id: string;
  name: string;
  enabled: boolean;
  expanded?: boolean;
  editing?: boolean;
  trigger: ScheduledTaskTrigger;
  action: ScheduledTaskAction;
  gate?: SchedulerGate;
  until?: string;
  runCount: number;
  lastRunAt?: string;
  /** 条件探针上一次检查的时刻（core state.lastProbe.at）：下次检查的锚点。 */
  lastProbeAt?: string;
  nextRunAt?: string;
  lastStatus: ScheduledTaskStatus;
  lastExitCode?: number | null;
  lastStdout?: string;
  lastStderr?: string;
  createdAt: string;
  updatedAt?: string;
};

export type SchedulerState = {
  runtimes: SchedulerRuntimePaths;
  tasks: ScheduledTask[];
};

export type AppState = {
  schemaVersion: number;
  nodes: AppNode[];
  tasks: Task[];
  selectedNodeId: string;
  backgrounds: Record<string, ListBackground>;
  /** 工具箱草稿纸（v0.8.4；v0.9.0 标签页化）：多页纯文本 + 激活页，跟着数据域同步（LWW 整份覆盖） */
  scratchpad: { tabs: ScratchpadTab[]; activeTabId: string; updatedAt: string };
  scheduler: SchedulerState;
};
