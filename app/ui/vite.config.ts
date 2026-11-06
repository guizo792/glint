import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
  resolve: process.env.VITEST ? { conditions: ["browser"] } : undefined,
  test: { environment: "jsdom", include: ["src/**/*.test.ts"] },
});
