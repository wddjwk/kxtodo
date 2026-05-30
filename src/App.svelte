<script lang="ts">
  import { onMount } from "svelte";
  import { matchesShortcut } from "./lib/shortcuts";
  import { buildAppShellStyle } from "./lib/styles";
  import {
    appSettings, showSettings, searchQuery,
    hydrate as hydrateStores
  } from "./lib/stores";
  import TitleBar from "./lib/TitleBar.svelte";
  import Toast from "./lib/Toast.svelte";
  import Sidebar from "./lib/Sidebar.svelte";
  import Workspace from "./lib/Workspace.svelte";
  import SettingsDrawer from "./lib/SettingsDrawer.svelte";

  let sidebarRef: Sidebar;
  let workspaceRef: Workspace;

  $: appShellStyle = buildAppShellStyle($appSettings.appearance);

  onMount(() => {
    void hydrateStores();
    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
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
<div class="app-shell" style={appShellStyle} on:click={closeOverlays}>
  <TitleBar />

  <div class="layout">
    <Sidebar bind:this={sidebarRef} />

    <Workspace bind:this={workspaceRef} />

    {#if $showSettings}
      <SettingsDrawer />
    {/if}
  </div>

  <Toast />
</div>
