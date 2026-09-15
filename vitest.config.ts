import { defineConfig } from "vitest/config";

/**
 * 单元测试配置（`npm run test:unit` → `vitest run`）。
 *
 * **刻意不复用 vite.config.ts**：vitest 的配置解析顺序是 `["vitest.config", "vite.config"]`
 * （见 node_modules/vitest/dist/chunks/constants.*.js 的 CONFIG_NAMES），本文件存在时
 * vite.config.ts 整个不参与——那里面有 svelte 插件、`optimizeDeps.include: ["mermaid"]`
 * （vitest 起服务时会真的去预打包 mermaid，纯逻辑单测白烧几十秒）与
 * `build.rollupOptions.output.manualChunks`（只对 `vite build` 有意义）。
 * vite.config.ts 本身一个字没动。
 *
 * `environment: "node"`：被测模块（ledger / clock / defaults）零 DOM 依赖，不装 jsdom。
 * defaults.ts 顶层会调 `@tauri-apps/plugin-os` 的 `platform()`，node 下它读
 * `window.__TAURI_OS_PLUGIN_INTERNALS__` 抛 ReferenceError，正好被 `isLinuxHost()`
 * 自己的 try/catch 接住并回落到 `navigator.userAgent`（Node 21+ 有这个全局），
 * 所以模块能正常求值。将来若要测碰 DOM 的模块（markdown.ts / images.ts），
 * 再单独给那个文件加 `// @vitest-environment jsdom` 或改这里的 environment。
 */
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/__tests__/**/*.spec.ts"]
  }
});
