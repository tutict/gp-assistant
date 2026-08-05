import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  build: {
    outDir: "../mobile-dist",
    emptyOutDir: true,
    manifest: true,
    sourcemap: false,
    cssCodeSplit: true,
    assetsInlineLimit: 2048,
    reportCompressedSize: true,
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            {
              name: "react",
              test: /node_modules[\\/](?:react|react-dom|scheduler)[\\/]/,
              priority: 20,
            },
            {
              name: "icons",
              test: /node_modules[\\/]lucide-react[\\/]/,
              priority: 15,
            },
          ],
        },
      },
    },
  },
  server: {
    proxy: {
      "/api": "http://127.0.0.1:8000",
      "/health": "http://127.0.0.1:8000",
    },
  },
});
