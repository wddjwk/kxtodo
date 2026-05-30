# Todo Note Agents Guide

## Product goal

Todo Note is a fast, local-first todo and quick-note app inspired by the productivity workflow of Microsoft To Do, while using original branding, assets, and implementation. It combines a hierarchical category/entry tree with live-rendered Markdown cards so an entry can be both an action item collection and a lightweight note.

## Core principles

- **Local-first and portable:** app data, settings, shortcuts, and exports live next to the executable when the directory is writable, with a safe development fallback.
- **Single-binary desktop first:** Windows is the primary target now. Linux and Android should remain possible through Tauri 2 without coupling core logic to Windows-only APIs.
- **Fast startup:** keep the frontend lean, persist JSON directly through Tauri commands, and avoid background services.
- **Original look with familiar ergonomics:** match the left-navigation/right-task-canvas workflow and soft card layout users expect, without copying proprietary Microsoft assets or branding.
- **Future sync-ready:** keep sync as an adapter boundary. Do not bake a cloud provider into the task model.

## Tech stack

- **Rust + Tauri 2** for the desktop shell, persistence, import/export, and future native integrations.
- **Svelte 4 + TypeScript + Vite** for the UI.
- **Markdown rendering** uses GitHub-flavored Markdown styling in the client, supporting headings, emphasis, highlight syntax, links, lists, task lists, inline code, and highlight.js-powered fenced code blocks with the GitHub light theme.

## Architecture (v6.0.0+)

The frontend follows a modular component architecture with centralized state management:

### State layer (`src/lib/stores.ts`)
- **Svelte writable stores** for `appState`, `appSettings`, `isHydrated`, `showSettings`, `searchQuery`, `toastMessage`.
- **Derived stores** for computed values: `selectedNode`, `listCounts`, `visibleTasks`, `accent`, `selectedBackground`, `isSearching`.
- `commit()` updates `appState` and queues a debounced save. `commitSettings()` does the same for settings, plus triggers native side-effects (zoom, autostart) when scale changes.
- `hydrate()` loads persisted data, syncs native appearance, registers global shortcut, and syncs lifecycle settings.
- The `isHydrated` guard prevents saves during initial load.

### Pure logic modules
- **`src/lib/nodes.ts`** — tree traversal and query functions: `descendantEntryIds`, `nodeAndDescendantIds`, `ancestorIds`, `tasksForNode`, `buildListCounts`, `buildVisibleTasks`, `moveTargetOptions`, `getBackground`, `exportStateForNode`. All functions are pure and take explicit arguments (no store access).
- **`src/lib/styles.ts`** — style computation: `buildAppShellStyle`, `buildSettingsDrawerStyle`, `buildMainStyle`, `buildMenuStyle`, `accentForNode`, `uiScaleValue`, `scalePercentValue`, `fontSizeValue`, `clampNumber`, `isNumberInRange`, `escapeCssUrl`, `avatarStyle`, `avatarInitial`.

### Components
- **`App.svelte`** — thin orchestrator (~60 lines). Mounts children, handles global keyboard shortcuts, coordinates overlay close across children.
- **`TitleBar.svelte`** — window title bar with minimize/maximize/close buttons.
- **`Toast.svelte`** — toast notification display.
- **`Sidebar.svelte`** — left panel: profile card, search box, system nav, custom tree (ListTree), tree context menu, icon picker, footer with new-entry/new-category buttons, resize handle. Owns local state: `renamingId`, `treeMenu`, `iconPickerListId`, `draggingId`, `sidebarWidth`.
- **`Workspace.svelte`** — main content area: list header, list menu (with background/theme/export/import), task list (TaskCard instances), task context menu, add-task bar. Owns local state: `newTaskDraft`, `selectedTaskId`, `showListMenu`, `taskMenu`, `showCompleted`.
- **`SettingsDrawer.svelte`** — settings panel: profile, appearance, lifecycle, shortcuts, cloud sync placeholder.
- **`TaskCard.svelte`** — individual task card with markdown rendering, editing, expand/collapse.
- **`ListTree.svelte`** — recursive tree component for custom nodes with drag/drop support.
- **`IconGlyph.svelte`** — renders Lucide icons or emoji characters.
- **`IconPicker.svelte`** — icon/emoji picker overlay.

### CSS architecture
Styles are split into 6 global CSS files imported in `src/main.ts` (cascade order matters):
- `src/styles/base.css` — root variables, resets, scrollbar, app-shell, layout.
- `src/styles/titlebar.css` — titlebar and window control buttons.
- `src/styles/sidebar.css` — sidebar, profile, search, nav, tree, icon picker, context menu.
- `src/styles/workspace.css` — workspace, list header, tasks, markdown, menus, composer.
- `src/styles/settings.css` — settings drawer.
- `src/styles/shared.css` — toast, utilities.

**Why global CSS, not Svelte scoped `<style>`:** Markdown content is rendered via `{@html}` and has no Svelte scope attributes. Scoped styles would not reach markdown elements. The `.collapse-button` class is used in both sidebar (tree categories) and workspace (task cards) with different styles, resolved by `.sidebar .collapse-button` and `.workspace .collapse-button` selectors.

### Existing modules (unchanged)
- `src/lib/types.ts` — data model types (`AppNode`, `Task`, `Settings`, `AppState`, `ListBackground`).
- `src/lib/defaults.ts` — default values, normalization, theme presets, node factory functions.
- `src/lib/backend.ts` — Tauri command bridge with browser-dev fallbacks.
- `src/lib/markdown.ts` — Markdown rendering and sanitization.
- `src/lib/shortcuts.ts` — keyboard shortcut matching.
- `src/lib/sync.ts` — sync adapter interface (placeholder).

## Project layout

```
src/
├── App.svelte              # Thin orchestrator
├── main.ts                 # Entry point, CSS imports
├── styles/
│   ├── base.css            # Root, resets, shell, layout
│   ├── titlebar.css        # Titlebar + window controls
│   ├── sidebar.css         # Sidebar + tree + icon picker
│   ├── workspace.css       # Tasks + markdown + menus
│   ├── settings.css        # Settings drawer
│   └── shared.css          # Toast, utilities
├── lib/
│   ├── stores.ts           # Centralized state + persistence
│   ├── nodes.ts            # Pure tree query functions
│   ├── styles.ts           # Style computation functions
│   ├── TitleBar.svelte     # Window controls
│   ├── Toast.svelte        # Toast notification
│   ├── Sidebar.svelte      # Left panel
│   ├── Workspace.svelte    # Main content area
│   ├── SettingsDrawer.svelte # Settings panel
│   ├── TaskCard.svelte     # Task card component
│   ├── ListTree.svelte     # Recursive tree component
│   ├── IconGlyph.svelte    # Icon rendering
│   ├── IconPicker.svelte   # Icon/emoji picker
│   ├── types.ts            # Data model types
│   ├── defaults.ts         # Defaults + normalization
│   ├── backend.ts          # Tauri bridge
│   ├── markdown.ts         # Markdown rendering
│   ├── shortcuts.ts        # Shortcut matching
│   └── sync.ts             # Sync adapter (placeholder)
src-tauri/
└── src/main.rs             # Rust backend
```

## Data model

- `AppNode` represents left-tree nodes. Built-in system nodes are `my-day`, `planned`, and `important`; custom `category` nodes are folders that expand/collapse; custom `entry` nodes own Markdown cards.
- `Task` belongs to an entry and stores Markdown content, completion state, My Day/favorite flags, lightweight date metadata, and transient expand/edit UI state. Collapsed cards render the first Markdown line with heading markers stripped; double-click toggles expanded rendering, and expanded/editing cards use the full middle column without reserving date space.
- `Settings` stores editable profile information (including uploaded avatar data URLs), display preferences (CSS UI scale, UI font size, Markdown base font size, editor font size, and link-opening mode), lifecycle preferences (close-to-tray and autostart), local shortcut bindings, the global toggle shortcut, and disabled cloud-sync configuration.
- The left tree is ordered by the `nodes` array. Drag/drop and the "移动到分组" menus must update both `parentId` and array position so classic reorder and reparent interactions stay predictable.
- Only one desktop instance should run. A second launch must focus the existing window; by default closing the window hides it to the tray, while the tray menu can reopen or exit the app.

## Technical quirks

- **UI Scale**: App uses `transform: scale(var(--ui-scale))` on `.app-shell` with `transform-origin: top left`. Internal dimensions use `calc(100 / scale)vw` which after scaling fit viewport. Configured via `buildAppShellStyle()`.
- **WebView2 quirk**: `position: fixed` elements outside the scaled `.app-shell` don't render when `#app` has `overflow: hidden`. Window controls must be inside the `.app-shell` container.
- **Single-instance**: `tauri_plugin_single_instance` uses app identifier `com.wddjwk.todonote`. Old exe files in release folder with same identifier will conflict — launching new version while old one runs causes focus of old window instead.
- **Portable data storage**: Uses `data_dir()` which resolves to `<exe_parent>/todo-note-data/` for portable deployment.
- **Autostart error**: `tauri_plugin_autostart` throws "file not found" (os error 2) when disabling and no shortcut exists. Handled by silently catching the error when `launchAtStartup` is false.

## Maintenance workflow

1. Install dependencies locally with `npm install`.
2. Run the desktop app with `npm run desktop:dev`.
3. Build the frontend with `npm run build`.
4. Package a release with `.\scripts\package.ps1 -Version 6.0.0`.
5. Load screenshot QA data with `.\scripts\load-sample-data.ps1` when needed; production defaults must stay minimal.
6. Keep new native features behind Tauri commands/plugins and keep sync integrations behind adapter-style boundaries.
