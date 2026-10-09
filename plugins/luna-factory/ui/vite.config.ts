import { defineConfig } from "vite";
import { singleFile } from "./tooling/singlefile";

export default defineConfig({
  plugins: [singleFile()],
  build: { target: "es2022", cssCodeSplit: false, assetsInlineLimit: 100_000_000, modulePreload: false, rollupOptions: { output: { inlineDynamicImports: true } } },
});
