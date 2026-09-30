import { defineProject } from "vitest/config";

export default defineProject({
  test: {
    name: "electron",
    environment: "node",
  },
});
