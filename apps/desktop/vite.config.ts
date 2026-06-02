import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 5173,
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: "./tests/setupTests.ts",
    css: true,
    // Default 5s is tight for Windows CI runners — the full App.tsx render
    // test routinely crosses that line on cold runs. Give every test 20s.
    testTimeout: 20_000,
    hookTimeout: 20_000,
  },
});
