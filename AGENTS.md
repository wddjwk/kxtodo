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
- **Svelte + TypeScript + Vite** for the UI.
- **Markdown rendering** uses GitHub-flavored Markdown styling in the client, supporting headings, emphasis, highlight syntax, links, lists, task lists, and inline code.

## Project layout

- `src/` contains the Svelte app.
- `src/lib/types.ts` defines the app data model shared by UI modules.
- `src/lib/backend.ts` is the frontend bridge to Tauri commands, with browser-dev fallbacks.
- `src/lib/markdown.ts` owns Markdown rendering and sanitization.
- `src-tauri/src/main.rs` owns local JSON persistence, export file writing, window-state support, and the global toggle shortcut.
- `scripts/package.ps1` builds a versioned release using local project dependencies.
- `scripts/load-sample-data.ps1` loads non-production sample data for visual validation.
- `test-data/sample-export.json` contains demo data used for screenshots and manual QA; do not move this into production defaults.

## Data model

- `AppNode` represents left-tree nodes. Built-in system nodes are `my-day`, `planned`, and `important`; custom `category` nodes are folders that expand/collapse; custom `entry` nodes own Markdown cards.
- `Task` belongs to an entry and stores Markdown content, completion state, My Day/favorite flags, lightweight date metadata, and transient expand/edit UI state.
- `Settings` stores editable profile information (including uploaded avatar data URLs), local shortcut bindings, the global toggle shortcut, and disabled cloud-sync configuration.

## Maintenance workflow

1. Install dependencies locally with `npm install`.
2. Run the desktop app with `npm run desktop:dev`.
3. Build the frontend with `npm run build`.
4. Package a release with `.\scripts\package.ps1 -Version 3.1.0`.
5. Load screenshot QA data with `.\scripts\load-sample-data.ps1` when needed; production defaults must stay minimal.
6. Keep new native features behind Tauri commands/plugins and keep sync integrations behind adapter-style boundaries.
