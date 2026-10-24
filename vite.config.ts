import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Tauri expects a fixed dev port and doesn't want Vite clearing its log output.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_ENV_"],
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] },
  },
  preview: { port: 1420, strictPort: true },
  build: {
    target: "es2022",
    outDir: "dist",
    sourcemap: false,
    // Local desktop bundle (loaded from disk, not the network).
    chunkSizeWarningLimit: 900,
  },
  test: {
    environment: "node",
    pool: "threads",
    isolate: false,
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
