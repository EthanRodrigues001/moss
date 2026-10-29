import { fileURLToPath } from "node:url";
import type { NextConfig } from "next";

// Moss ships inside Tauri, so the UI is a static export with no server.
const nextConfig: NextConfig = {
  output: "export",
  // Lets a check build run next to a running `tauri dev` without sharing its .next folder.
  distDir: process.env.MOSS_DIST_DIR ?? ".next",
  images: { unoptimized: true },
  trailingSlash: true,
  turbopack: {
    // The repo root also has a package.json (Tauri CLI); pin Turbopack to the UI folder.
    root: fileURLToPath(new URL(".", import.meta.url)),
    rules: {
      "*.css": {
        loaders: ["@tailwindcss/turbopack"],
        as: "*.css",
      },
    },
  },
};

export default nextConfig;
