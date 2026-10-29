// Build `moss-mcp` and place it where Tauri bundles it from, named the way Tauri expects for a
// sidecar (`<name>-<target triple>.exe`). The installer puts it next to Moss.exe as `moss-mcp.exe`,
// which is where the app looks for it (src-tauri/src/agents.rs).
//
//   node scripts/prepare-sidecar.mjs            debug build (fast, for `tauri dev`)
//   node scripts/prepare-sidecar.mjs --release  release build (for the installer)
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const release = process.argv.includes("--release");
const triple = "x86_64-pc-windows-msvc";

const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, stdio: ["ignore", "pipe", "inherit"] });

const buildArgs = ["build", "-p", "moss-mcp", ...(release ? ["--release"] : [])];
execFileSync("cargo", buildArgs, { cwd: root, stdio: "inherit" });

const meta = JSON.parse(run("cargo", ["metadata", "--format-version", "1", "--no-deps"]).toString());
const built = join(meta.target_directory, release ? "release" : "debug", "moss-mcp.exe");
if (!existsSync(built)) {
  console.error(`moss-mcp.exe was not found at ${built}`);
  process.exit(1);
}
const dest = join(root, "src-tauri", "binaries");
mkdirSync(dest, { recursive: true });
copyFileSync(built, join(dest, `moss-mcp-${triple}.exe`));
console.log(`moss-mcp -> src-tauri/binaries/moss-mcp-${triple}.exe`);
