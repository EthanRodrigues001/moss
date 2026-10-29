// One version for the whole app, kept equal in:
//   Cargo.toml (workspace.package.version), package.json, ui/package.json, src-tauri/tauri.conf.json
//
//   node scripts/bump-version.mjs 0.1.0-beta.2   set all four
//   node scripts/bump-version.mjs --check        fail if they differ (CI runs this)
//   node scripts/bump-version.mjs --check v0.1.0-beta.2   also fail if it differs from a tag
//
// Cargo.lock follows on the next `cargo check`.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;

const jsonFiles = ["package.json", "ui/package.json", "src-tauri/tauri.conf.json"];
const cargoFile = "Cargo.toml";

function cargoVersion(text) {
  const m = text.match(/\[workspace\.package\][^[]*?\nversion\s*=\s*"([^"]+)"/);
  if (!m) throw new Error("no [workspace.package] version in Cargo.toml");
  return m[1];
}

function read(rel) {
  return readFileSync(join(root, rel), "utf8");
}

function versions() {
  const found = { [cargoFile]: cargoVersion(read(cargoFile)) };
  for (const f of jsonFiles) found[f] = JSON.parse(read(f)).version;
  return found;
}

const args = process.argv.slice(2);

if (args[0] === "--check") {
  const found = versions();
  const wanted = args[1] ? args[1].replace(/^v/, "") : Object.values(found)[0];
  const bad = Object.entries(found).filter(([, v]) => v !== wanted);
  for (const [f, v] of Object.entries(found)) console.log(`${v === wanted ? "ok " : "BAD"} ${f}: ${v}`);
  if (bad.length) {
    console.error(`Versions differ from ${wanted}. Run: node scripts/bump-version.mjs ${wanted}`);
    process.exit(1);
  }
  process.exit(0);
}

const next = (args[0] ?? "").replace(/^v/, "");
if (!SEMVER.test(next)) {
  console.error("Usage: node scripts/bump-version.mjs <version, e.g. 0.1.0-beta.2> | --check [tag]");
  process.exit(2);
}

for (const f of jsonFiles) {
  // Replace only the first top-level "version" so the file's formatting is untouched.
  const text = read(f);
  const updated = text.replace(/("version"\s*:\s*")[^"]+(")/, `$1${next}$2`);
  writeFileSync(join(root, f), updated);
}
const cargo = read(cargoFile).replace(
  /(\[workspace\.package\][^[]*?\nversion\s*=\s*")[^"]+(")/,
  `$1${next}$2`,
);
writeFileSync(join(root, cargoFile), cargo);

const found = versions();
for (const [f, v] of Object.entries(found)) console.log(`${v === next ? "ok " : "BAD"} ${f}: ${v}`);
if (Object.values(found).some((v) => v !== next)) process.exit(1);
console.log("Now run `cargo check` so Cargo.lock follows.");
