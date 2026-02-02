import { resolve } from "node:path";
import vue from "@vitejs/plugin-vue";
import { defineConfig } from "vite";

const cacheDir = process.env.CODE_WEBUI_CACHE_DIR || process.env.VITE_CACHE_DIR;

export default defineConfig({
  plugins: [vue()],
  base: "/assets/",
  cacheDir,
  server: {
    cors: true,
    headers: {
      "Access-Control-Allow-Origin": "*",
      "Access-Control-Allow-Headers": "*",
    },
  },
  build: {
    outDir: resolve(__dirname, "../gateway/src/assets"),
    emptyOutDir: true,
    assetsDir: "",
    cssCodeSplit: false,
    rollupOptions: {
      output: {
        entryFileNames: "app-core.js",
        chunkFileNames: "app-core.js",
        inlineDynamicImports: true,
        assetFileNames: (assetInfo) => {
          if (assetInfo.name === "style.css") {
            return "app.css";
          }
          return assetInfo.name || "asset";
        },
      },
    },
  },
});
