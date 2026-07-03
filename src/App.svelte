<script lang="ts">
  import { onMount } from "svelte";
  import { matchesShortcut } from "./lib/shortcuts";
  import { buildAppShellStyle, buildMobileShellStyle } from "./lib/styles";
  import {
    appSettings, showSettings, searchQuery,
    hydrate as hydrateStores
  } from "./lib/stores";
  import { isMobile, mobileView } from "./lib/platform";
  import { startSchedulerRuntime } from "./lib/scheduler";
  import TitleBar from "./lib/TitleBar.svelte";
  import Toast from "./lib/Toast.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import Workspace from "./lib/Workspace.svelte";
  import SettingsDrawer from "./lib/SettingsDrawer.svelte";

  let sidebarRef: Sidebar;
  let workspaceRef: Workspace;

  $: appShellStyle = $isMobile
    ? buildMobileShellStyle($appSettings.appearance)
    : buildAppShellStyle($appSettings.appearance);

  onMount(() => {
    let stopScheduler: () => void = () => undefined;
    void hydrateStores().then(() => {
      stopScheduler = startSchedulerRuntime();
    });
    window.addEventListener("keydown", handleShortcut);
    return () => {
      stopScheduler();
      window.removeEventListener("keydown", handleShortcut);
    };
  });

  function closeOverlays(): void {
    if (sidebarRef?.shouldSuppressClose()) return;
    sidebarRef?.closeOverlays();
    workspaceRef?.closeOverlays();
    showSettings.set(false);
  }

  function handleShortcut(event: KeyboardEvent): void {
    if (matchesShortcut(event, $appSettings.shortcuts.focusSearch)) {
      event.preventDefault();
      sidebarRef?.focusSearch();
    } else if (matchesShortcut(event, $appSettings.shortcuts.newTask)) {
      event.preventDefault();
      workspaceRef?.focusComposer();
    } else if (matchesShortcut(event, $appSettings.shortcuts.openSettings)) {
      event.preventDefault();
      showSettings.update((v) => !v);
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="app-shell"
  class:mobile={$isMobile}
  class:view-list={$isMobile && $mobileView === "list"}
  class:view-content={$isMobile && $mobileView === "content"}
  style={appShellStyle}
  on:click={closeOverlays}
>
  {#if !$isMobile}
    <TitleBar />
  {/if}

  <div class="layout">
    <Sidebar bind:this={sidebarRef} />

    <Workspace bind:this={workspaceRef} />

    {#if $showSettings}
      <SettingsDrawer />
    {/if}
  </div>

  <Toast />
</div>
