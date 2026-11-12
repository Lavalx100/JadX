import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  test: {
    // The IDE validation fixture is a standalone Node project with its own
    // `node --test` runner; don't load those tests into JadX's Vitest runtime.
    exclude: ["**/node_modules/**", "**/dist/**", "**/.idea/**", "**/.git/**", "**/.cache/**", "fixtures/**"],
  },
  server: {
    port: 1420,
    strictPort: true,
  },
});
