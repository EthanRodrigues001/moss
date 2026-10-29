# Moss

Local-first Windows desktop LaTeX editor: Tauri v2 + static-exported Next.js (shadcn/ui, Monaco, Tiptap) over a Rust core. Compiles with the user's own TeX install, syncs with Overleaf (free plan, unofficial session sync), and exposes everything to AI agents through `moss-mcp`.

## Read first
- `GLOSSARY.md` for the domain language. Use those exact terms in code, UI copy, MCP tool names and docs.
- `docs/adr/` for decisions that are hard to reverse. Do not undo one without writing a superseding ADR.
- `docs/ROADMAP.md` for the build order and what is done.

## Keep the docs alive (required)
- When a domain term is resolved or renamed, update `GLOSSARY.md` in the same change. It holds language only, never implementation detail.
- Add an ADR only when a decision is hard to reverse, surprising without context, and a real trade-off. Number sequentially in `docs/adr/`.
- Tick milestones in `docs/ROADMAP.md` as they land.

## Old code
`D:\Ethan\Projects\moss` is the previous web version. Treat it as a read-only quarry for good pieces (editor, equation editor, lint, PDF preview). Do not copy Supabase, SwiftLaTeX, Render or the fake HTML compiler.

## Installed skills
Project skills live in `.agents/skills/` (Rust best practices, async, testing, Tauri v2, Rust MCP server). Use them for Rust, Tauri and MCP work.

## Tooling gotchas (Windows)
- Rust, TeX and LaTeX sources contain many backslashes. Create such files with the file tools, not shell heredocs, which can drop or mangle them.
- Run Cargo from the repo root (`cargo test -p moss-core`). Tauri CLI: `npm run tauri dev`.
- Monaco, MathLive fonts and the PDF.js worker are copied into `ui/public/vendor` by `ui/scripts/copy-vendor.mjs` (runs before `dev` and `build`; git-ignored). To check a production build while `tauri dev` runs: `MOSS_DIST_DIR=.next-check npx next build` in `ui/` (output lands in that folder).
