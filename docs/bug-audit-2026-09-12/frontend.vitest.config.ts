import { defineConfig } from "vitest/config";
import { fileURLToPath } from "node:url";

export default defineConfig({
  resolve: {
    alias: { "@": fileURLToPath(new URL("../../src", import.meta.url)) },
  },
  test: {
    environment: "jsdom",
    include: [
      "src/store/player/regressions.test.ts",
      "src/hooks/usePlayback.test.ts",
    ],
    fileParallelism: false,
  },
});
