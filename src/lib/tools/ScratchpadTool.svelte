<script lang="ts">
  /**
   * 草稿纸（需求 2）：一块纯文本便签。不渲染、不解析、不做任何 markdown 处理——
   * 「跟记事本一样」就是它的全部规格：一个铺满的 textarea，标准的输入体验。
   * 内容自动保存（防抖 400ms；切后台 / 关页面前 flush 一次）。
   *
   * 存 localStorage 而不是领域文件：草稿纸不是待办 / 日记 / 账目那样的领域数据，
   * 不进同步、不进备份、不给 CLI；为它单开一个领域文件是过度设计。
   * 写失败（配额满）静默吞掉：便签不该弹报错打断输入。
   */
  import { onDestroy, onMount } from "svelte";

  const STORAGE_KEY = "kxtodo-scratchpad-v1";
  const SAVE_DEBOUNCE_MS = 400;

  let text = "";
  let saved = true;
  let timer: number | undefined;

  function flush(): void {
    window.clearTimeout(timer);
    try {
      // 值没变一个字节都不写：同步 setItem 是同步磁盘 I/O
      if (localStorage.getItem(STORAGE_KEY) !== text) localStorage.setItem(STORAGE_KEY, text);
      saved = true;
    } catch {
      saved = false;
    }
  }

  function handleInput(): void {
    saved = false;
    window.clearTimeout(timer);
    timer = window.setTimeout(flush, SAVE_DEBOUNCE_MS);
  }

  function onVisibility(): void {
    if (document.visibilityState === "hidden") flush();
  }

  onMount(() => {
    try {
      text = localStorage.getItem(STORAGE_KEY) ?? "";
    } catch {
      text = "";
    }
    window.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("pagehide", flush);
  });

  onDestroy(() => {
    window.removeEventListener("visibilitychange", onVisibility);
    window.removeEventListener("pagehide", flush);
    flush();
  });
</script>

<div class="scratchpad">
  <div class="scratchpad-status">
    <span>{saved ? "已自动保存" : "正在输入…"}</span>
    <span>{text.length} 字</span>
  </div>
  <textarea
    class="scratchpad-area"
    bind:value={text}
    on:input={handleInput}
    placeholder="随手记点什么…（纯文本，不渲染，自动保存）"
    spellcheck="false"
  ></textarea>
</div>
