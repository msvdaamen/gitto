import { defineConfig } from "drizzle-kit";

export default defineConfig({
  dialect: "sqlite",
  schema: "../../packages/db/src/schema.ts",
  // Shipped with the packaged app as an extra resource (see forge.config.ts).
  out: "./migrations",
});
