import path from "node:path";

import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects the dev server on a known port + HMR over a separate
// websocket port that lets the macOS native window reach the Vite
// process. Numbers are arbitrary but must agree with tauri.conf.json.
const TAURI_PORT = 1420;
const HMR_PORT = 1421;

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
    },
  },
  clearScreen: false,
  server: {
    port: TAURI_PORT,
    strictPort: true,
    host: process.env.TAURI_DEV_HOST ?? false,
    hmr: process.env.TAURI_DEV_HOST
      ? {
          protocol: "ws",
          host: process.env.TAURI_DEV_HOST,
          port: HMR_PORT,
        }
      : undefined,
    watch: {
      // Don't choke on watching the Rust target/ tree.
      ignored: ["**/src-tauri/**", "**/target/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: process.env.TAURI_PLATFORM === "windows" ? "chrome105" : "safari16",
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
    outDir: "dist",
    emptyOutDir: true,
    // Push the heavy editor + highlighter + flow chunks into their
    // own files. They're only needed when the corresponding panel
    // mounts, and they cache cleanly across builds.
    rollupOptions: {
      output: {
        manualChunks(id: string) {
          if (id.includes("monaco-editor")) return "monaco";
          if (id.includes("shiki")) return "shiki";
          if (id.includes("reactflow") || id.includes("/dagre/")) {
            return "reactflow";
          }
          if (id.includes("xterm")) return "xterm";
        },
      },
    },
    chunkSizeWarningLimit: 4000,
  },
});
