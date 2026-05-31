<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Minus, Square, X } from "@lucide/svelte";
  import { showToast } from "./stores";
  import logoUrl from "../../logo.png";

  async function minimizeWindow(): Promise<void> {
    try {
      await getCurrentWindow().minimize();
    } catch (error) {
      showToast(`最小化失败：${String(error)}`);
    }
  }

  async function toggleMaximizeWindow(): Promise<void> {
    try {
      await getCurrentWindow().toggleMaximize();
    } catch (error) {
      showToast(`切换最大化失败：${String(error)}`);
    }
  }

  async function closeWindow(): Promise<void> {
    try {
      await getCurrentWindow().close();
    } catch (error) {
      showToast(`关闭窗口失败：${String(error)}`);
    }
  }
</script>

<header class="titlebar" data-tauri-drag-region>
  <div class="window-title" data-tauri-drag-region>
    <img class="app-glyph" src={logoUrl} alt="KXToDo" draggable="false" />
    <span>KXToDo</span>
  </div>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="window-controls" on:click|stopPropagation>
    <button type="button" aria-label="最小化" on:click={minimizeWindow}><Minus size={16} /></button>
    <button type="button" aria-label="最大化" on:click={toggleMaximizeWindow}><Square size={14} /></button>
    <button type="button" aria-label="关闭" on:click={closeWindow}><X size={16} /></button>
  </div>
</header>
