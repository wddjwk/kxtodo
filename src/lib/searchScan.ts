/**
 * 全局搜索的分块扫描器（v0.8.6 需求 1）。
 *
 * 为什么不再用 derived：15MB 文本的同步重算会卡死界面，而 derived 的求值是同步的、
 * 无法分片。扫描器把「扫」与「渲染」解耦——每批约 1000 条，跑在
 * `requestIdleCallback`（带 timeout 兜底）或 `setTimeout` 上，防抖词一变就作废上一轮。
 *
 * 语义：
 * - **清空立即生效**：空词同步回空结果，不走 idle（宿主还会先 cancel 上一轮）；
 * - 中间批次是「扫到哪儿显示到哪儿」（渐进上屏），**完成后的最后一批**才全局按
 *   `updatedAt` 排序并截到 `SEARCH_HIT_LIMIT` 条——中间批不排序是刻意的，每批都排
 *   一遍会让最坏情况（宽词命中上万条）的代价变成 O(命中数²/批大小)；
 * - 匹配规则复用三个领域模块的预折叠索引（`diaryFold` / `ledgerMatches` / `taskFold`
 *   + `matchingNodeIds`），不另写一份。
 */
import type { AppState, CardStyle, DiaryEntry, LedgerBook, SearchHit, Task } from "./types";
import { diaryFold } from "./diary";
import { ledgerMatches } from "./ledger";
import { matchingNodeIds, taskFold } from "./nodes";

/** 每批扫描的条目数（任务 + 日记 + 流水按同一个游标推进） */
export const SEARCH_CHUNK = 1000;
/** 结果封顶：命中再多也只渲染这么多（列表本身走 VirtualStack，这里封的是数据量） */
export const SEARCH_HIT_LIMIT = 200;

export type SearchSource = { state: AppState; diaries: DiaryEntry[]; ledger: LedgerBook };
export type SearchScanner = { run: (query: string) => void; cancel: () => void };

export type SearchScannerOptions = {
  /** 扫描开始的那一刻取一份数据快照（分片期间不再重读，语义与「一次快照一次结果」一致） */
  source: () => SearchSource;
  /** 每批把当前结果交出去；`done` 为 true 的那批是排序 + 封顶后的最终结果 */
  onBatch: (hits: SearchHit[], done: boolean) => void;
  /** 分片调度；测试注入同步实现 */
  schedule?: (task: () => void) => void;
};

type IdleDeadline = { didTimeout: boolean; timeRemaining: () => number };
type IdleWindow = Window & {
  requestIdleCallback?: (callback: (deadline: IdleDeadline) => void, options?: { timeout: number }) => number;
};

/** idle + timeout 双保险：timeout 到点就算没空也会跑，塞满帧时不会把扫描饿死。 */
export function idleSchedule(task: () => void): void {
  const idle = (window as IdleWindow).requestIdleCallback;
  if (typeof idle === "function") {
    idle(() => task(), { timeout: 120 });
    return;
  }
  // 旧 WebKit（Linux 桌面 WebKitGTK / 老的 safari13 target）没有 requestIdleCallback
  window.setTimeout(task, 0);
}

const touched = (item: { updatedAt?: string; createdAt: string }): string => item.updatedAt || item.createdAt;

function stampOf(hit: SearchHit): string {
  return hit.kind === "task" ? touched(hit.task) : touched(hit.entry);
}

export function createSearchScanner(options: SearchScannerOptions): SearchScanner {
  const schedule = options.schedule ?? idleSchedule;
  let token = 0;

  return {
    cancel() {
      token += 1;
    },
    run(query: string) {
      const mine = ++token;
      const needle = query.trim().toLowerCase();
      if (!needle) {
        options.onBatch([], true);
        return;
      }
      const { state, diaries, ledger } = options.source();
      const cardStyleByNode = new Map<string, CardStyle>(
        state.nodes.filter((node) => node.cardStyle === "card").map((node) => [node.id, "card"])
      );
      const nameHits = matchingNodeIds(state, needle);
      const tasks = state.tasks;
      const entries = ledger.entries;
      const total = tasks.length + diaries.length + entries.length;
      const hits: SearchHit[] = [];
      let cursor = 0;

      const hitAt = (index: number): SearchHit | null => {
        if (index < tasks.length) {
          const task: Task = tasks[index];
          if (!taskFold(task).includes(needle) && !nameHits.has(task.nodeId)) return null;
          return {
            kind: "task",
            key: `task-${task.id}`,
            task,
            cardStyle: cardStyleByNode.get(task.nodeId) ?? "todo"
          };
        }
        const diaryIndex = index - tasks.length;
        if (diaryIndex < diaries.length) {
          const entry = diaries[diaryIndex];
          if (!diaryFold(entry).includes(needle)) return null;
          return { kind: "diary", key: `diary-${entry.id}`, entry };
        }
        const entry = entries[index - tasks.length - diaries.length];
        if (!ledgerMatches(ledger, entry, needle)) return null;
        return { kind: "ledger", key: `ledger-${entry.id}`, entry };
      };

      const step = (): void => {
        if (mine !== token) return;
        const end = Math.min(cursor + SEARCH_CHUNK, total);
        for (; cursor < end; cursor += 1) {
          const hit = hitAt(cursor);
          if (hit) hits.push(hit);
        }
        if (cursor < total) {
          options.onBatch(hits.slice(), false);
          schedule(step);
          return;
        }
        options.onBatch(
          [...hits].sort((a, b) => stampOf(b).localeCompare(stampOf(a))).slice(0, SEARCH_HIT_LIMIT),
          true
        );
      };

      schedule(step);
    }
  };
}
