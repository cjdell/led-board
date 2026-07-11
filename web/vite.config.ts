import { defineConfig } from "vite";
import solidPlugin from "vite-plugin-solid";
import devtools from "solid-devtools/vite";
import deno from "@deno/vite-plugin";

export default defineConfig({
  plugins: [devtools(), solidPlugin(), deno()],
  server: {
    allowedHosts: true,
    host: "0.0.0.0",
    port: 3000,
    hmr: false,
  },
  resolve: {
    alias: {
      "@components": "/src/components",
      "@lib": "/src/lib",
    },
  },
  build: {
    target: "esnext",
  },
});
