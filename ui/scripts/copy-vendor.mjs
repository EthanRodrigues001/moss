// Copies the files Monaco and PDF.js load at run time into `public/vendor`, so the desktop app
// never fetches anything from a CDN. Runs before `dev` and `build`; the output is git-ignored.

import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "public", "vendor");

const monacoSrc = join(root, "node_modules", "monaco-editor", "min", "vs");
const mathliveFonts = join(root, "node_modules", "mathlive", "fonts");
const pdfWorkerSrc = join(root, "node_modules", "pdfjs-dist", "build", "pdf.worker.min.mjs");

for (const src of [monacoSrc, pdfWorkerSrc, mathliveFonts]) {
  if (!existsSync(src)) {
    console.error(`copy-vendor: missing ${src}. Run \`npm install\` in ui/ first.`);
    process.exit(1);
  }
}

rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, "monaco"), { recursive: true });
cpSync(monacoSrc, join(out, "monaco", "vs"), { recursive: true });
cpSync(mathliveFonts, join(out, "mathlive", "fonts"), { recursive: true });
cpSync(pdfWorkerSrc, join(out, "pdf.worker.min.mjs"));
console.log("copy-vendor: Monaco, MathLive fonts and the PDF.js worker are in public/vendor");
