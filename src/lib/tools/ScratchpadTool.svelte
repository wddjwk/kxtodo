<script lang="ts">
  /**
   * 草稿纸（v0.8.4 需求 2 改造后）：一块纯文本便签。不渲染、不解析、不做任何
   * markdown 处理——「跟记事本一样」就是它的全部规格。
   *
   * **存储与同步**：正文住在数据域（`data.json` 的 `scratchpad`），跟着「同步数据」
   * 范围一起走（LWW 整段覆盖），换台设备打开就还在。localStorage 只留作**首帧兜底**
   * （水合前的第一帧先画缓存里的字，别让用户看见空屏）。
   *
   * **保存时机**：防抖 5 秒 + 切后台/关页面前 flush——每次落盘都是一条审计 + 一次
   * revision，逐键写会灌爆台账；真到 5 秒时用户多半已经停手，代价只是「拔电源前
   * 最后几秒的字没落盘」。
   */
  import { get } from "svelte/store";
  import { onDestroy, onMount } from "svelte";
  import { appState } from "../stores";
  import { setScratchpad } from "../actions";

  const STORAGE_KEY = "kxtodo-scratchpad-v1";
  const SAVE_DEBOUNCE_MS = 5000;

  let text = "";
  let timer: number | undefined;
  let seeded = false;
  /** 有未 flush 的输入（水合到来的覆盖要避让，见下面的 seed 分支） */
  let dirty = false;

  function flush(): void {
    window.clearTimeout(timer);
    timer = undefined;
    dirty = false;
    try {
      // 值没变一个字节都不写：同步 setItem 是同步磁盘 I/O
      if (localStorage.getItem(STORAGE_KEY) !== text) localStorage.setItem(STORAGE_KEY, text);
    } catch {
      // 配额满：缓存失败无所谓，正文已经进领域文件
    }
    if (get(appState).scratchpad.text === text) return;
    void setScratchpad(text);
  }

  function handleInput(): void {
    dirty = true;
    window.clearTimeout(timer);
    timer = window.setTimeout(flush, SAVE_DEBOUNCE_MS);
  }

  function handleKeydown(event: KeyboardEvent): void {
    if (event.key !== "Tab" || event.shiftKey || event.ctrlKey || event.altKey || event.metaKey || event.isComposing) return;
    event.preventDefault();
    document.execCommand("insertText", false, "\t");
  }

  function onVisibility(): void {
    if (document.visibilityState === "hidden") flush();
  }

  onMount(() => {
    // 首帧：先画缓存里的字（水合还没来）。已经 seed 过就别拿缓存盖回去
    // ——store 已经是权威，缓存可能更旧。
    if (!seeded) {
      try {
        text = localStorage.getItem(STORAGE_KEY) ?? "";
      } catch {
        text = "";
      }
    }
    window.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("pagehide", flush);
  });

  // 数据域（含同步拉回来的）是权威：水合/同步一到就覆盖本地缓存那一份，
  // 但**只覆盖一次**——之后用户在页面上的输入不能被它拉回去。
  $: if (!seeded && $appState.scratchpad.updatedAt) {
    seeded = true;
    // 水合到达时用户可能已经打了字、防抖还没 flush：脏输入优先。
    // 无提示地拿旧值盖掉刚打的字，是比「晚几秒合并」严重得多的伤害。
    if (!dirty) text = $appState.scratchpad.text;
  }

  onDestroy(() => {
    window.removeEventListener("visibilitychange", onVisibility);
    window.removeEventListener("pagehide", flush);
    flush();
  });
</script>

<div class="scratchpad">
  <textarea
    class="scratchpad-area"
    bind:value={text}
    on:input={handleInput}
    on:keydown={handleKeydown}
    placeholder="随手记点什么…（纯文本，不渲染，自动保存）"
    spellcheck="false"
  ></textarea>
  <!-- 字数统计：需求 2 只要求隐藏「正在输入/已自动保存」，计数留着（v0.8.5 需求 6） -->
  <div class="scratchpad-foot"><span class="scratchpad-count">{text.length} 字</span></div>
</div>
