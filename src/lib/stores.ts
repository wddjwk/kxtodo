import { writable, derived, get } from "svelte/store";
import type { AppNotification, AppState, AppNode, DiaryEditorTarget, DiaryEntry, EmojiPickerTarget, LedgerBook, LedgerEditorTarget, LedgerSide, NotificationTone, SchedulerState, Settings, Task } from "./types";
import { cachedAppearance, cachedFeatures, cachedProfile, cachedState, defaultSchedulerRuntimes, defaultSettings, emptyState, normalizeDiaryEntries, normalizeLedger, normalizeState, normalizeSettings, schedulerRuntimeKeys, seedLedgerBook, writeAppearanceCache, writeFeaturesCache, writeProfileCache, writeStateCache } from "./defaults";
import {
  loadState, saveState, loadSettings, saveSettings, loadScheduler, saveScheduler,
  loadDiary, saveDiary, loadLedger, saveLedger,
  registerGlobalShortcut, setCloseToTray, setAutostart,
  setWebviewZoom, isTauriRuntime, resolveExecutorPaths, sendNativeNotification,
  hasCoreDispatch, coreSnapshot, getAppVersion
} from "./backend";
import { buildListCounts, buildSearchHits, buildVisibleTasks, getBackground } from "./nodes";
import { accentForNode, uiScaleValue } from "./styles";
import { entryToUi, type ScheduleEntryV9 } from "./scheduleAdapter";
import { weekStartIndex } from "./diary";
import { caps } from "./capabilities";

/** 运行时版本号：构建期由 build.rs 从 git tag/commit 注入（KXTODO_VERSION），hydrate 时填充。 */
export const appVersion = writable("");


/**
 * 与同步服务器的连接状态。来源是后台同步循环/探测写进 `runtime/sync.json` 的缓存，
 * 面板只读它——打开设置界面绝不触发阻塞式网络探测。
 * `online === null` 表示还没探测过（刚装好或从未同步）。
 */
export type SyncConnection = {
  online: boolean | null;
  lastSeenAt?: string | null;
  lastError?: string | null;
  checking?: boolean;
};

export const syncConnection = writable<SyncConnection>({ online: null });

/**
 * 下一次自动同步的时间戳（毫秒）。由 syncRunner 排程时写入，`null` = 没有排程
 * （未配对或已暂停）。设置面板据此显示倒计时，用户能直接核对节奏是否与设定一致。
 */
export const nextSyncAt = writable<number | null>(null);

/**
 * 手动同步（设置面板「立即同步」、移动端下拉/菜单）完成的时刻。
 * 自动同步循环订阅它：手动跑过一轮后，下一轮从这一刻重新计一个完整间隔，
 * 而不是沿用旧排程（否则手动同步完几秒后自动循环又插一轮，节奏看着就是乱的）。
 */
export const manualSyncAt = writable(0);

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

export function now(): string {
  return new Date().toISOString();
}

export function todayIso(): string {
  const date = new Date();
  const offset = date.getTimezoneOffset();
  const local = new Date(date.getTime() - offset * 60_000);
  return local.toISOString().slice(0, 10);
}

export function yesterdayIso(): string {
  const date = new Date();
  date.setDate(date.getDate() - 1);
  const offset = date.getTimezoneOffset();
  const local = new Date(date.getTime() - offset * 60_000);
  return local.toISOString().slice(0, 10);
}

export function dateOnly(value?: string): string | undefined {
  return value?.slice(0, 10) || undefined;
}

export function createTaskId(): string {
  if (crypto.randomUUID) return `task-${crypto.randomUUID().replace(/-/g, "")}`;
  return `task-${Math.random().toString(16).slice(2, 10)}${Math.random()
    .toString(16)
    .slice(2, 10)}${Math.random().toString(16).slice(2, 10)}${Math.random()
    .toString(16)
    .slice(2, 10)}`;
}

export function createDiaryId(): string {
  if (crypto.randomUUID) return `diary-${crypto.randomUUID().replace(/-/g, "")}`;
  return `diary-${Math.random().toString(16).slice(2, 10)}${Math.random()
    .toString(16)
    .slice(2, 10)}${Math.random().toString(16).slice(2, 10)}${Math.random()
    .toString(16)
    .slice(2, 10)}`;
}

export function safeFileName(name: string): string {
  return name.replace(/[<>:"/\\|?*\u0000-\u001F]/g, "-").slice(0, 80) || "todo-note";
}

export function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error ?? new Error("读取文件失败"));
    reader.readAsDataURL(file);
  });
}

// ---------------------------------------------------------------------------
// Core stores
// ---------------------------------------------------------------------------

/**
 * appState 的第一帧就用上次退出时缓存的界面状态（节点树/任务/选中节点/背景）：
 * 水合是异步的，等 core 快照回来再渲染，冷启动就会先闪一帧「收集箱 + 空列表」
 * （安卓 WebView 被系统回收后，解锁亮屏也是一次冷启动，同样闪）。缓存没水合出的
 * 数据新鲜度差通常在百毫秒级，快照到了自然对账——绝大多数时候两边一字不差。
 */
export const appState = writable<AppState>(cachedState() ?? emptyState());
// 首帧就用上一次退出时缓存的外观（缩放/字号）：水合是异步的，等设置回来再应用
// 会先按默认缩放渲染一帧再跳变（安卓上观感是「卡卡的、不稳定」）
const initialSettings = clone(defaultSettings);
Object.assign(initialSettings.appearance, cachedAppearance());
Object.assign(initialSettings.profile, cachedProfile());
// 特性开关也进首帧（理由见 cachedFeatures）：状态缓存让卡片第一帧就画出来，
// 开关若还是默认值，临期底色与链接样式都会「先按默认画一遍再改回来」。
initialSettings.features = cachedFeatures();
export const appSettings = writable<Settings>(initialSettings);
appSettings.subscribe((settings) => {
  writeAppearanceCache(settings.appearance);
  writeProfileCache(settings.profile);
  writeFeaturesCache(settings.features);
});
/**
 * 一周从周几开始（0 = 周日，1 = 周一）。设置项 `features.weekStart` 归一后的值，
 * 日历视图、日期选择器、周统计与「本周」分组统一读它——**不许各自去读设置**，
 * 否则同一个界面里会出现两个不同的「周一」。
 */
export const weekStart = derived(appSettings, ($settings) => weekStartIndex($settings.features.weekStart));
/**
 * 日记（diary.json，独立的第四个领域）。跟 scheduleEntries 一样单独成 store：
 * 它有自己的 revision 与域事件，塞进 appState 会让「只刷新日记」变成刷新整个数据域。
 */
export const diaryEntries = writable<DiaryEntry[]>([]);
export const isHydrated = writable(false);

/**
 * 首帧状态缓存的写入：防抖 800ms（每次勾选/编辑都同步写一份几 MB 的 JSON 会把
 * 交互拖垮——写缓存自己绝不能成为新的卡顿源），内容没变一个字节都不写；
 * 退到后台（visibilitychange/pagehide）立刻补写一次——安卓随时可能杀进程，
 * 缓存最多陈旧「最后一次防抖窗口」这么久。只在**水合完成后**写：水合前 store 里
 * 是缓存种子本身，回写没有意义，core 初始化失败时更不许拿它盖掉好数据。
 * （这一块必须声明在 isHydrated 之后：subscribe 回调在订阅那一刻就会跑一次。）
 */
let stateCacheTimer: number | undefined;
let lastStateCacheJson = "";

function writeStateCacheNow(state: AppState): void {
  window.clearTimeout(stateCacheTimer);
  stateCacheTimer = undefined;
  const { scheduler: _scheduler, ...rest } = state;
  let json = "";
  try {
    json = JSON.stringify(rest);
  } catch {
    return;
  }
  if (json === lastStateCacheJson) return;
  if (writeStateCache(state)) lastStateCacheJson = json;
}

function scheduleStateCache(state: AppState): void {
  window.clearTimeout(stateCacheTimer);
  stateCacheTimer = window.setTimeout(() => writeStateCacheNow(state), 800);
}

appState.subscribe((state) => {
  if (!get(isHydrated)) return;
  scheduleStateCache(state);
});

// 水合完成本身也要写一次：appState 的 set 发生在 isHydrated 翻真**之前**，
// 那次订阅回调被门控挡掉——不补这一笔，「启动后什么都不动就退出」的会话
// 永远留着上上次的缓存。
isHydrated.subscribe((ready) => {
  if (ready) scheduleStateCache(get(appState));
});

if (typeof document !== "undefined" && typeof window !== "undefined") {
  const flushStateCache = (): void => {
    if (!get(isHydrated)) return;
    writeStateCacheNow(get(appState));
  };
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") flushStateCache();
  });
  window.addEventListener("pagehide", flushStateCache);
}

export const showSettings = writable(false);
export const searchQuery = writable("");
export const taskEmojiPicker = writable<EmojiPickerTarget | null>(null);
/** 正在浮窗编辑器中编辑的任务 ID（null = 编辑器关闭）。 */
export const editorTaskId = writable<string | null>(null);

/**
 * 编辑器的「新建事项」模式：存的是归属条目 id（与 editorTaskId 互斥）。
 * 保存时才调 task.add，正文为空就直接关掉——点了加号又改主意的，
 * 不该留下一条空任务。
 */
export const editorDraftNode = writable<string | null>(null);

/** 日记视图是否打开（桌面端）。移动端由 `mobileView === "diary"` 驱动，见 platform.ts。 */
export const diaryOpen = writable(false);

/**
 * 正在编辑的日记：`{ id }` 改已有的一篇，`{ date }` 新建一篇（**保存时才落盘**，
 * 于是关掉一个空草稿不会留下一篇空日记）。null = 编辑器关闭。
 */
export const diaryEditor = writable<DiaryEditorTarget | null>(null);

/**
 * 记账（ledger.json，独立的第五个领域）。整本账一个 store：账户/分类/流水
 * 总是一起变（记一笔只动流水，但余额与统计要立刻跟着算），拆三个 store 只会
 * 让视图层自己拼一致性。
 */
export const ledgerData = writable<LedgerBook>({
  accounts: [],
  categories: [],
  entries: [],
  accountTypes: []
});
/** 记账视图是否打开（桌面端）。移动端由 `mobileView === "ledger"` 驱动，见 platform.ts。 */
export const ledgerOpen = writable(false);
/** 工具箱是否占着主区域（桌面端，v0.7.5）。移动端由 `mobileView === "toolbox"` 驱动。 */
export const toolboxOpen = writable(false);
/**
 * 正在记的那一笔：`{ id }` 改已有的一笔，`{ date, kind }` 新记一笔（保存时才落盘）。
 * null = 面板关闭。
 */
export const ledgerEditor = writable<LedgerEditorTarget | null>(null);
/**
 * 记账面板里点了分类加号：请记账页打开分类管理的新增表单（可带预置的大类）。
 * 面板挂在 App 层、分类管理长在 LedgerView 上，两棵不相干的子树只能靠 store 递话。
 */
export const ledgerCategoryDraft = writable<{ side: LedgerSide; parentId: string } | null>(null);

// ---------------------------------------------------------------------------
// Toast
// ---------------------------------------------------------------------------

export const toastMessage = writable("");
let toastTimer: number | undefined;

export function showToast(message: string, durationMs = 3200): void {
  toastMessage.set(message);
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => toastMessage.set(""), durationMs);
}

export function showNotification(
  message: string,
  options: Partial<Omit<AppNotification, "message">> = {}
): Promise<void> {
  const settings = get(appSettings);
  const notification: AppNotification = {
    title: options.title?.trim() || "KXToDo",
    message: message.trim() || "通知",
    durationMs: Math.min(60_000, Math.max(1_200, Math.round(options.durationMs ?? settings.notifications.durationMs))),
    tone: (options.tone ?? "info") as NotificationTone,
    position: options.position ?? settings.notifications.position
  };

  if (!isTauriRuntime) {
    showToast(`${notification.title}：${notification.message}`, notification.durationMs);
    return Promise.resolve();
  }

  if (caps.systemNotifications) {
    // 移动端与 Linux 桌面：走 tauri-plugin-notification 的系统通知；任何失败降级为 Toast。
    void (async () => {
      try {
        const { isPermissionGranted, requestPermission, sendNotification } = await import("@tauri-apps/plugin-notification");
        let granted = await isPermissionGranted();
        if (!granted) {
          granted = (await requestPermission()) === "granted";
        }
        if (granted) {
          await sendNotification({ title: notification.title, body: notification.message });
        } else {
          showToast(`${notification.title}：${notification.message}`, notification.durationMs);
        }
      } catch (error) {
        showToast(`通知发送失败：${String(error)}`);
      }
    })();
    return Promise.resolve();
  }

  void sendNativeNotification(notification).catch((error) => {
    showToast(`通知发送失败：${String(error)}`);
  });
  return Promise.resolve();
}

// ---------------------------------------------------------------------------
// Derived stores
// ---------------------------------------------------------------------------

export const systemNodes = derived(appState, ($s) =>
  $s.nodes.filter((n) => n.kind === "system")
);

export const firstEntry = derived(appState, ($s) =>
  $s.nodes.find((n) => n.kind === "entry")
);

export const selectedNode = derived(
  [appState, firstEntry, systemNodes],
  ([$s, $fe, $sn]) =>
    $s.nodes.find((n) => n.id === $s.selectedNodeId) ?? $fe ?? $sn[0]
);

export const listCounts = derived(appState, ($s) => buildListCounts($s));

/** 搜索的防抖副本。
 *  全局搜索每敲一个键都要扫任务 + 日记全文 + 全部流水，而 `visibleTasks` 与 `searchHits`
 *  都依赖搜索词 —— 一个键跑两遍全量匹配（安卓上尤其明显）。
 *  **清空立刻生效**：退出搜索态、移动端返回键清词、点条目清词都不能等一拍。
 *  订阅时的当前值也立刻透传，否则「带着搜索词切换订阅者」会先拿到空值。 */
const SEARCH_DEBOUNCE_MS = 180;

export const debouncedSearchQuery = writable(get(searchQuery));

let searchDebounceTimer: ReturnType<typeof setTimeout> | undefined;
let searchDebouncePrimed = false;
// 应用生命周期级别的订阅（与 stores 里其它模块级订阅同款），不需要退订。
searchQuery.subscribe((value) => {
  if (searchDebounceTimer !== undefined) {
    clearTimeout(searchDebounceTimer);
    searchDebounceTimer = undefined;
  }
  if (!searchDebouncePrimed || value.trim() === "") {
    searchDebouncePrimed = true;
    debouncedSearchQuery.set(value);
    return;
  }
  searchDebounceTimer = setTimeout(() => {
    searchDebounceTimer = undefined;
    debouncedSearchQuery.set(value);
  }, SEARCH_DEBOUNCE_MS);
});

export const visibleTasks = derived(
  [appState, selectedNode, debouncedSearchQuery],
  ([$s, $node, $q]) => buildVisibleTasks($s, $node, $q)
);

export const selectedBackground = derived(
  [selectedNode, appState],
  ([$node, $s]) => getBackground($node?.id, $s.backgrounds)
);

export const accent = derived(
  [selectedNode, appSettings],
  ([$node, $settings]) => accentForNode($node, $settings.appearance.uiColors)
);

// isSearching 跟着防抖后的词走：否则界面会先切进搜索态、结果还在 180ms 之后才到，
// 中间那一帧是「无搜索结果」的空态，每敲一个键闪一次。
export const isSearching = derived(debouncedSearchQuery, ($q) => $q.trim().length > 0);

/**
 * 全局搜索的混排结果（任务 + 日记 + 记账，按最近改动排序）。
 * 桌面在工作区渲染，移动端在侧栏搜索框下方的结果面板渲染——同一份数据，两处视图。
 */
export const searchHits = derived(
  [appState, diaryEntries, ledgerData, debouncedSearchQuery],
  ([$s, $d, $l, $q]) => buildSearchHits($s, $d, $l, $q)
);

// ---------------------------------------------------------------------------
// Legacy persistence（浏览器预览路径；桌面与移动端 Tauri 走 actions.ts 命令化写入）
// ---------------------------------------------------------------------------

let stateSaveTimer: number | undefined;
let settingsSaveTimer: number | undefined;
let schedulerSaveTimer: number | undefined;
let diarySaveTimer: number | undefined;
let ledgerSaveTimer: number | undefined;

export function commit(next: AppState): void {
  appState.set(next);
  if (!get(isHydrated)) return;
  window.clearTimeout(stateSaveTimer);
  stateSaveTimer = window.setTimeout(() => {
    saveState(next).catch((error) => showToast(`保存失败：${String(error)}`));
  }, 180);
}

export function commitDiary(next: DiaryEntry[]): void {
  diaryEntries.set(next);
  if (!get(isHydrated)) return;
  window.clearTimeout(diarySaveTimer);
  diarySaveTimer = window.setTimeout(() => {
    saveDiary(next).catch((error) => showToast(`保存日记失败：${String(error)}`));
  }, 180);
}

export function commitLedger(next: LedgerBook): void {
  ledgerData.set(next);
  if (!get(isHydrated)) return;
  window.clearTimeout(ledgerSaveTimer);
  ledgerSaveTimer = window.setTimeout(() => {
    saveLedger(next).catch((error) => showToast(`保存账本失败：${String(error)}`));
  }, 180);
}

export function commitScheduler(next: SchedulerState): void {
  appState.update((state) => ({ ...state, scheduler: next }));
  if (!get(isHydrated)) return;
  window.clearTimeout(schedulerSaveTimer);
  schedulerSaveTimer = window.setTimeout(() => {
    saveScheduler(next).catch((error) => showToast(`保存定时任务失败：${String(error)}`));
  }, 180);
}

export function commitSettings(next: Settings): void {
  const previousScale = uiScaleValue(get(appSettings).appearance.uiScale);
  const nextScale = uiScaleValue(next.appearance.uiScale);
  appSettings.set(next);
  if (!get(isHydrated)) return;
  window.clearTimeout(settingsSaveTimer);
  settingsSaveTimer = window.setTimeout(() => {
    saveSettings(next).catch((error) => showToast(`保存设置失败：${String(error)}`));
  }, 180);
  if (previousScale !== nextScale) {
    void syncNativeAppearance(next);
  }
}

// ---------------------------------------------------------------------------
// Native sync
// ---------------------------------------------------------------------------

async function syncNativeAppearance(nextSettings: Settings): Promise<void> {
  if (!isTauriRuntime) return;
  try {
    await setWebviewZoom(uiScaleValue(nextSettings.appearance.uiScale));
  } catch (error) {
    showToast(`界面缩放同步失败：${String(error)}`);
  }
}

async function syncNativeLifecycle(nextSettings: Settings): Promise<void> {
  try {
    await setCloseToTray(nextSettings.lifecycle.closeToTray);
  } catch (error) {
    showToast(`系统设置同步失败：${String(error)}`);
  }
  try {
    await setAutostart(nextSettings.lifecycle.launchAtStartup);
  } catch (error) {
    if (nextSettings.lifecycle.launchAtStartup) {
      showToast(`开机自启设置失败：${String(error)}`);
    }
  }
}

// ---------------------------------------------------------------------------
// v9 core mode：快照刷新 + 领域事件 + 编辑冲突策略（§4.3）
// ---------------------------------------------------------------------------

/** v9 ScheduleEntry → 侧表（GUI 编辑器据此构建 patch，保留 CLI 专属字段）。 */
export const scheduleEntries = writable<Map<string, ScheduleEntryV9>>(new Map());

/** 编辑基准：itemId → 开始编辑时的 updatedAt（用于保存冲突检测）。 */
const editBases = new Map<string, string | undefined>();

export function markEditStart(task: Task): void {
  editBases.set(task.id, task.updatedAt);
}

export function clearEditBase(taskId: string): void {
  editBases.delete(taskId);
}

export function rebaseEditBase(taskId: string, updatedAt?: string): void {
  if (editBases.has(taskId)) {
    editBases.set(taskId, updatedAt);
  }
}

export function editBaseUpdatedAt(taskId: string): string | undefined {
  return editBases.get(taskId);
}

export let coreMode = false;

/** 每个域已经落进 store 的 revision。
 *  域事件带着 revision 来，而写操作之后前端往往已经显式 refresh 过一次 ——
 *  没有这道判断，同一次写入就是两轮完整的快照往返（读盘 + 序列化 + IPC + 全量 normalize）。 */
const appliedRevisions = new Map<string, number>();

/**
 * 写命令的信封自带落盘后的 revision（`meta.revisionDomain` / `meta.revision`）。
 * 前端已经乐观更新过 store 的写入（config.set、gui.set-*-ui）把它记下来，紧随其后的
 * 域事件就被水位判断挡住——省掉一整轮快照往返，更要紧的是省掉「同一个值换一次对象
 * 身份」引发的全树响应式重跑：那一轮要是落在饼图入场动画中间，就是肉眼可见的掉帧。
 */
export function noteEnvelopeRevision(meta: Record<string, unknown> | undefined): void {
  const domain = meta?.revisionDomain;
  const revision = meta?.revision;
  if (typeof domain !== "string" || typeof revision !== "number") return;
  appliedRevisions.set(domain, Math.max(appliedRevisions.get(domain) ?? 0, revision));
}

function noteRevisions(snapshot: Awaited<ReturnType<typeof coreSnapshot>>): void {
  const revisions = snapshot.revisions;
  if (!revisions) return;
  for (const [domain, revision] of Object.entries(revisions)) {
    if (typeof revision === "number") appliedRevisions.set(domain, revision);
  }
}

function applySnapshot(snapshot: Awaited<ReturnType<typeof coreSnapshot>>, domains?: Set<string>): void {
  const wantAll = !domains;
  // 先留一份旧水位再记新的：只有**真的前进了**的域才值得换一次对象身份。
  // 写命令信封记过水位（noteEnvelopeRevision）而 store 里已经是同一个值的，
  // 这一份快照整体跳过——身份不变，订阅者一个都不惊动。
  const previous = new Map(appliedRevisions);
  noteRevisions(snapshot);
  const advanced = (domain: "data" | "settings" | "diary" | "ledger" | "schedule"): boolean => {
    const incoming = snapshot.revisions?.[domain];
    // 快照没带这个域的 revision（不该发生）就保守照旧应用
    if (typeof incoming !== "number") return true;
    return incoming > (previous.get(domain) ?? 0);
  };
  // 每个分支都要判 `snapshot.<域>` 存在：快照现在按域裁剪，没请求的域不在载荷里。
  if ((wantAll || domains?.has("data")) && snapshot.data && advanced("data")) {
    const current = get(appState);
    const normalized = normalizeState({
      ...snapshot.data,
      scheduler: current.scheduler
    });
    appState.set({ ...normalized, scheduler: current.scheduler });
  }
  if ((wantAll || domains?.has("settings")) && snapshot.settings !== undefined && advanced("settings")) {
    appSettings.set(normalizeSettings(snapshot.settings));
  }
  if ((wantAll || domains?.has("diary")) && snapshot.diary !== undefined && advanced("diary")) {
    diaryEntries.set(normalizeDiaryEntries(snapshot.diary));
  }
  if ((wantAll || domains?.has("ledger")) && snapshot.ledger !== undefined && advanced("ledger")) {
    ledgerData.set(normalizeLedger(snapshot.ledger));
  }
  if ((wantAll || domains?.has("schedule")) && snapshot.schedule && advanced("schedule")) {
    const current = get(appState);
    const entries = snapshot.schedule.tasks as ScheduleEntryV9[];
    scheduleEntries.set(new Map(entries.map((entry) => [entry.id, entry])));
    const runtimes = schedulerRuntimeKeys.reduce((acc, key) => {
      acc[key] = snapshot.schedule?.runtimes?.[key] || "";
      return acc;
    }, { ...defaultSchedulerRuntimes });
    appState.set({
      ...get(appState),
      scheduler: {
        runtimes,
        tasks: entries.map(entryToUi)
      }
    });
  }
}

let snapshotInFlight = false;
let snapshotPending = false;
let pendingAllDomains = false;
const pendingDomains = new Set<string>();

function queueSnapshot(domains?: string[]): void {
  snapshotPending = true;
  if (!domains) {
    pendingAllDomains = true;
    pendingDomains.clear();
    return;
  }
  if (!pendingAllDomains) {
    domains.forEach((domain) => pendingDomains.add(domain));
  }
}

/** 从 Host 拉取最新快照；in-flight 期间的事件合并后立即再拉取。 */
export async function refreshFromCore(domains?: string[]): Promise<void> {
  if (!coreMode) return;
  queueSnapshot(domains);
  if (snapshotInFlight) return;
  snapshotInFlight = true;
  try {
    while (snapshotPending) {
      const all = pendingAllDomains;
      const requested = all ? undefined : new Set(pendingDomains);
      // 域列表要在清空之前取走：快照按域裁剪，请求的就是这几个域。
      const wanted = all ? undefined : [...pendingDomains];
      snapshotPending = false;
      pendingAllDomains = false;
      pendingDomains.clear();
      try {
        const snapshot = await coreSnapshot(wanted);
        applySnapshot(snapshot, requested);
      } catch {
        // 快照现在是**按域裁剪**的，所以失败的那几个域必须重新排回去：不然下一次 refresh
        // 只拉它自己的域，这次失败的域就永远停在旧数据上（要等下一次全量水合才修得好）。
        // 直接升级成「下次拉全部」——出错时保守一点，比漏掉一个域便宜。
        // 仍然 break：不在这里紧凑重试，后续任意领域事件都会重新触发。
        pendingAllDomains = true;
        pendingDomains.clear();
        snapshotPending = true;
        break;
      }
    }
  } finally {
    snapshotInFlight = false;
  }
}

async function listenCoreEvents(): Promise<void> {
  const { listen } = await import("@tauri-apps/api/event");
  await listen<{ domain?: string; revision?: number }>("kxtodo://domain-changed", (event) => {
    const domain = event.payload?.domain;
    const revision = event.payload?.revision;
    // 这一版已经在手里了就别再拉一次：写操作的路径通常是「前端乐观更新 → dispatch →
    // 显式 refreshFromCore」，而 Host 的域事件在 dispatch 内部就已经发出来了，
    // 两边撞在一起就是同一次写入两轮完整快照。
    //
    // `revision > 0` 这个前提不能省：**revision 0 是「没有版本号，强制刷新」的哨兵**——
    // CLI 改同步配置（`sync register/unpair/configure/now`）走的是
    // `ops_sync::notify_settings_changed`，它没有写 settings 因而拿不到 revision，
    // 只能发 0 来叫醒常驻的 GUI。而 `appliedRevisions.get(domain) ?? 0` 对 0 恒为真，
    // 不加这道守卫那些事件会被无条件丢掉：GUI 常驻时 CLI 改了同步配置，
    // 设置面板与自动同步循环要等到下一次无关事件才回刷。
    if (domain && typeof revision === "number" && revision > 0 && (appliedRevisions.get(domain) ?? 0) >= revision) {
      return;
    }
    void refreshFromCore(domain ? [domain] : undefined);
  });
}

/**
 * 提醒 / 通知的两条事件通路（v0.8.3）。
 *
 * - `kxtodo://notification`：**移动端**的通知。core 的 `MobileBackend` 不自绘通知窗
 *   （那是桌面的独立 always-on-top 窗口），而是把载荷交回前端，由 `showNotification`
 *   走系统通知插件（含权限申请）并在失败时降级 Toast——移动端的通知呈现只留一处实现，
 *   Rust 侧再抄一份权限逻辑就是两套口径。桌面不会收到这个事件。
 * - `kxtodo://reminder-error`：提醒引擎的故障（台账损坏、通知发不出去）。这类问题
 *   只有用户能修（比如去系统设置里开通知权限），写进日志没人看得见。
 */
async function listenReminderEvents(): Promise<void> {
  const { listen } = await import("@tauri-apps/api/event");
  await listen<Partial<AppNotification>>("kxtodo://notification", (event) => {
    const payload = event.payload;
    if (!payload) return;
    void showNotification(payload.message ?? "", {
      title: payload.title,
      durationMs: payload.durationMs,
      tone: payload.tone,
      position: payload.position
    });
  });
  await listen<{ code?: string; message?: string }>("kxtodo://reminder-error", (event) => {
    showToast(`提醒没能触发：${event.payload?.message ?? "未知错误"}`, 8000);
  });
}

// ---------------------------------------------------------------------------
// Hydration
// ---------------------------------------------------------------------------

export async function hydrate(): Promise<void> {
  // 启动路径上本来串行的三段（版本号 IPC → core 探测 IPC → 事件模块动态 import）并行发出去：
  // 它们互不依赖，串起来就是在首屏数据到达之前白等三个往返。
  // **事件监听本身仍然必须在快照之前注册完成**（否则会漏掉这期间发生的域事件），
  // 这里只预热它的 chunk —— 真正的 listen 还在下面的 coreMode 分支里。
  const versionReady = getAppVersion()
    .then((version) => appVersion.set(version))
    .catch(() => {
      // 版本号缺失不阻塞启动
    });
  const coreReady = hasCoreDispatch();
  void import("@tauri-apps/api/event").catch(() => undefined);
  try {
    coreMode = await coreReady;
  } catch (error) {
    // Desktop capability failures fail closed: never enter legacy full-save mode.
    coreMode = true;
    isHydrated.set(true);
    showToast(`Domain Core 初始化失败，已禁止写入以保护数据：${String(error)}`, 10_000);
    return;
  }
  await versionReady;
  if (coreMode) {
    // 两条监听都必须在拉快照之前注册完成（否则会漏掉这期间发生的事件）；
    // 它们互不依赖，并行发出去。
    await Promise.all([listenCoreEvents(), listenReminderEvents()]);
    let snapshotLoaded = false;
    try {
      const snapshot = await coreSnapshot();
      applySnapshot(snapshot);
      snapshotLoaded = true;
    } catch (error) {
      showToast(`数据加载失败，已禁止 legacy 覆盖；请运行 kxtodo doctor：${String(error)}`, 10_000);
    } finally {
      isHydrated.set(true);
    }
    if (!snapshotLoaded) return;
    const settings = get(appSettings);
    await syncNativeAppearance(settings);
    try {
      await registerGlobalShortcut(settings.shortcuts.toggleWindow);
    } catch (error) {
      showToast(`全局快捷键注册失败：${String(error)}`);
    }
    await syncNativeLifecycle(settings);
    return;
  }

  // 浏览器预览路径（移动端已是 core 模式）
  let loadedSettings = clone(defaultSettings);
  try {
    const [storedState, storedScheduler, storedSettings, storedDiary, storedLedger, resolvedExecutors] = await Promise.all([
      loadState(),
      loadScheduler(),
      loadSettings(),
      loadDiary(),
      loadLedger(),
      resolveExecutorPaths().catch(() => defaultSchedulerRuntimes)
    ]);
    diaryEntries.set(storedDiary);
    const storedLedgerBook = normalizeLedger(storedLedger);
    if (storedLedgerBook.accounts.length === 0 && storedLedgerBook.entries.length === 0) {
      // 浏览器预览的首跑也要有种子账户/分类，与 core 的 ensure_initialized 同口径
      const seeded = seedLedgerBook();
      ledgerData.set(seeded);
      saveLedger(seeded).catch(() => undefined);
    } else {
      ledgerData.set(storedLedgerBook);
    }
    const scheduler: SchedulerState = {
      ...storedScheduler,
      runtimes: schedulerRuntimeKeys.reduce((acc, key) => {
        acc[key] = storedScheduler.runtimes[key] || resolvedExecutors[key] || "";
        return acc;
      }, { ...storedScheduler.runtimes })
    };
    appState.set({
      ...normalizeState(storedState),
      scheduler
    });
    const normalizedSettings = normalizeSettings(storedSettings);
    appSettings.set(normalizedSettings);
    loadedSettings = normalizedSettings;
  } catch {
    // First launch or corrupted data — silently use defaults
  } finally {
    isHydrated.set(true);
  }

  await syncNativeAppearance(loadedSettings);

  try {
    await registerGlobalShortcut(loadedSettings.shortcuts.toggleWindow);
  } catch (error) {
    showToast(`全局快捷键注册失败：${String(error)}`);
  }

  await syncNativeLifecycle(loadedSettings);
}
