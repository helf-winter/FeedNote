import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";

export default defineConfig({
  root: resolve(__dirname),
  plugins: [react()],
  publicDir: resolve(__dirname, "public"),
  build: {
    outDir: resolve(__dirname, "../dist-web"),
    emptyOutDir: true,
  },
  server: {
    host: "127.0.0.1",
    port: 4173,
    strictPort: true,
    fs: { allow: [resolve(__dirname, "..")] },
    proxy: { "/api": "http://127.0.0.1:4174" },
  },
});
