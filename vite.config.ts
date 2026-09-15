import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST ?? "127.0.0.1";

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],
  optimizeDeps: {
    // mermaid 的 esm 入口在 dev 下靠运行时动态 import 自己的 chunks，vite 边跑边
    // 优化会触发整页 reload / 404，图永远停在「渲染中」。预打包成单份依赖就稳了。
    include: ["mermaid"]
  },
  server: {
    host,
    port: 1420,
    strictPort: true
  },
  build: {
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
    rollupOptions: {
      output: {
        // KaTeX 有两个引用方（markdown.ts 的公式渲染、mermaid 内部的数学标签），
        // rollup 的默认启发式会把它**打两份**：一份进首屏 entry，一份 261KB 的独立 chunk。
        // 强制收成一个共享 chunk，两边都 import 它。其余模块一律交回默认拆分
        // （返回 undefined），别去干扰 mermaid 那套已经调好的懒加载 chunk。
        manualChunks(id: string): string | undefined {
          if (id.includes("node_modules/katex/")) return "katex";
          return undefined;
        }
      }
    }
  }
});
