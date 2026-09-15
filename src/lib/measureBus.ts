/**
 * 共享的「元素尺寸变化」总线。
 *
 * 每张卡片早先各建自己的 ResizeObserver + window resize 监听：300 张卡 = 300 个 observer
 * + 300 个 window 监听，而且同一个节点被两条通路各触发一次（一次窗口缩放 = 600 次回调），
 * 每个回调里都要读 scrollWidth / getComputedStyle —— 那是强制同步布局，拖动窗口时
 * resize 事件按 60Hz 来，等于每秒几万次重排。
 *
 * 这里换成：**单例 observer + 单个 window 监听 + rAF 合帧派发**。
 * - ResizeObserver 的回调带着 entries，只通知尺寸真的变了的那些元素；
 * - window resize 无法知道谁变了，通知全部，但同一帧内只跑一遍；
 * - 两条通路都并进同一个 rAF，于是「一次窗口缩放」的布局读取集中在一帧里做完。
 *
 * **ResizeObserver 不能省**（只留 window resize 会退化）：移动端首屏卡片在 view-list 下是
 * `display:none`，挂载时量到的全是 0，点进内容页变可见时**没有任何 window 事件**，
 * 只有 RO 能接到（v0.6.3 的折叠态省略号就栽在这）。
 */

type Listener = () => void;

const listeners = new Map<Element, Set<Listener>>();
/** 这一帧要通知的元素；`notifyAll` 为真时表示「整窗都变了，全都量一遍」。 */
const pending = new Set<Element>();
let notifyAll = false;
let frame = 0;
let observer: ResizeObserver | null = null;
let windowBound = false;

function flush(): void {
  frame = 0;
  const targets = notifyAll ? [...listeners.keys()] : [...pending];
  pending.clear();
  notifyAll = false;
  for (const element of targets) {
    const set = listeners.get(element);
    if (!set) continue;
    // 复制一份再遍历：listener 里可能间接导致订阅/退订
    for (const listener of [...set]) listener();
  }
}

function schedule(): void {
  if (frame !== 0) return;
  if (typeof requestAnimationFrame !== "function") {
    flush();
    return;
  }
  frame = requestAnimationFrame(flush);
}

function handleWindowResize(): void {
  if (listeners.size === 0) return;
  notifyAll = true;
  schedule();
}

function ensureObserver(): void {
  if (observer !== null || typeof ResizeObserver === "undefined") return;
  observer = new ResizeObserver((entries) => {
    let touched = false;
    for (const entry of entries) {
      if (listeners.has(entry.target)) {
        pending.add(entry.target);
        touched = true;
      }
    }
    if (touched) schedule();
  });
}

/**
 * 订阅一个元素的尺寸变化。返回退订函数。
 * 订阅时**不会**立刻回调一次——调用方自己在挂载时量一遍（那一次必须是同步的，
 * 首帧就要有正确结论）。
 */
export function observeResize(element: Element, listener: Listener): () => void {
  ensureObserver();
  let set = listeners.get(element);
  if (set === undefined) {
    set = new Set();
    listeners.set(element, set);
    observer?.observe(element);
  }
  set.add(listener);
  if (!windowBound && typeof window !== "undefined") {
    windowBound = true;
    window.addEventListener("resize", handleWindowResize);
  }
  let released = false;
  return () => {
    if (released) return;
    released = true;
    const current = listeners.get(element);
    if (current !== undefined) {
      current.delete(listener);
      if (current.size === 0) {
        listeners.delete(element);
        pending.delete(element);
        observer?.unobserve(element);
      }
    }
    if (listeners.size === 0 && windowBound) {
      windowBound = false;
      window.removeEventListener("resize", handleWindowResize);
    }
  };
}
