# Work phases (resume file)

The user compacts the conversation after each phase to save tokens. Do ONE phase at a time, tick it here when done, add a "Done notes" line, then stop and say which phase is next. Read this file first when resuming.

Standing rules: Windows; create backslash-heavy files with Write/Edit (no shell heredocs); run cargo from the repo root; build/test with `CARGO_TARGET_DIR=D:/cargo-target/moss` (C: is nearly full); no CDN; commit/push nothing without asking; NEVER delete without explicit permission; no foreground sleep; update GLOSSARY.md, docs/adr/, docs/ROADMAP.md when relevant.

## Phase 0 - Loose ends from the compile fix (needs the user's yes)
- [x] `compile.rs`: per-Build-cache file lock (`BuildLock`, `compile.lock`) and `taskkill /T /F` on timeout; `Error::Compile` added; `cargo test -p moss-core compile` passed.
- [x] Told the user about the "Page dictionary kid reference" error (corrupt PDF in `.moss\build\0Synopsis\`). On 2026-10-28 the PDF had already been rebuilt (6.6 MB, 11:12), so no deletion was needed. Likely cause: two compiles on one build folder, or a killed run; now guarded by `compile.lock`. `compile.lock` (0 bytes) stays in the folder by design.
- [x] User said "go forward" on `CARGO_TARGET_DIR`: user env var set to `D:/cargo-target/moss` (new terminals pick it up).
- [x] Deleted `target\debug` contents (about 22.8 GB freed). Still left: `target\debug\moss-app.exe` and `moss-mcp.exe` (~42 MB, locked by the running `tauri dev`). Delete them (and the `target` folder) after `tauri dev` is closed, then restart it; the next build goes to `D:/cargo-target/moss`.
- [ ] Tick the compile-lock note in docs/ROADMAP.md; optionally add a BuildLock test and run clippy.

## Phase 1 - Fix the Overleaf modal (small)
Bug (screenshot): in the Overleaf import dialog a long project title ("Decentralized Manufacturing ... Architecture") does not wrap or truncate, so the row grows wider than the modal. The description line, filter box and the right-hand "Import / Import again" and "Test write" buttons are clipped by the dialog edge, and the dialog has no usable horizontal fit.
- [x] Done in `ui/components/moss/overleaf-dialog.tsx`: replaced `ScrollArea` (its content wrapper grew to the widest row) with a plain `overflow-y-auto overflow-x-hidden` div, title truncates with a tooltip, badge and buttons `shrink-0`, `DialogContent` is `overflow-hidden` with `[&>*]:min-w-0`.
- [ ] Not yet checked visually in the running app (needs `tauri dev` restart/hot reload); typecheck not run.

## Phase 2 - PDF preview A: text layer, links, back (`ui/components/moss/pdf-viewer.tsx`)
- [x] pdf.js text layer (`pdfjs.TextLayer` + `pdfjs-dist/web/pdf_viewer.css`) so text can be selected and copied.
- [x] Own link layer from `page.getAnnotations()` (Link subtype): internal links resolve named/explicit destinations (XYZ/FitH height) and scroll; web/mail links call new Tauri command `open_external` (`chrome_bridge.rs`, only http/https/mailto, via `rundll32 url.dll,FileProtocolHandler`) through `api.openExternal`.
- [x] Back button + Alt+Left (stack of scroll positions, max 50).
- Done notes: `tsc` and `cargo check -p moss-app` pass. NOT tried in the running app: restart `tauri dev` (new Rust command), then check selection/copy, a contents-page link, a bibliography link, a URL link, Back. Double-click inverse search now also selects a word (text layer); links stop the double-click.
- Keep: lazy pages, zoom, fit width, SyncTeX flash, double-click inverse search, Follow cursor.

## Phase 3 - PDF preview B: navigation and file access
- [x] Page box + "12 / 40" indicator (current page from scroll position).
- [x] Find in PDF: Ctrl+F while the PDF pane has focus (click it first) or the search button; own text search over `getTextContent`, yellow boxes, current hit orange, Enter / Shift+Enter, "n of m", Esc closes.
- [x] Outline sidebar (`getOutline`), toggle button shown only when the PDF has an outline, collapsible tree, click jumps (and feeds Back).
- [x] Save PDF as / Open PDF: Tauri commands `pdf_save_as`, `pdf_open` in `src-tauri/src/lib.rs`; `PdfViewer` takes a `project` prop (passed by `PreviewPane` from `moss-app.tsx`).
- Done notes: `tsc`, eslint and `cargo check -p moss-app` pass. NOT tried in the running app (restart `tauri dev`). Find highlight boxes are estimated from text-piece widths, so they can be a little off on ligatures or math; hits split across lines get one box per piece.

## Phase 4 - Project search and replace
Spec from the user: a Ctrl+F-style search over all characters in all files of the Project, fast; list the files in which it was found; styled like the Add project / Overleaf modal (they liked it a lot); with Replace and Replace all.
- [x] Rust `moss-core/src/search.rs`: `search` and `replace` over text files on up to 8 threads; options case, whole word, regex, file globs (`*.tex, chapters/**, !old`); per-file counts plus line snippets with match ranges (long lines cut around the match); skips binaries, `.moss/`, generated files, files over 4 MB. Matching is per line so a counted match is exactly what a replace changes. Patterns that match empty text are refused.
- [x] `replace` takes ONE history restore point first (`Before replacing "x" with "y"`), writes nothing when there is nothing to change; `$1` groups work in regex mode only.
- [x] Tauri commands `search_project`, `replace_in_project` (`src-tauri/src/search.rs`), `api.searchProject` / `api.replaceInProject`.
- [x] UI `ui/components/moss/search-dialog.tsx` in the Add-project palette style: query box with Aa / whole word / regex / replace toggles, Files filter, results grouped by file with counts and highlighted lines, arrow keys + Enter, click opens the file at the line, "Replace in file" and "Replace all" (inline confirm showing the count). Ctrl+Shift+F or the header search button; prefilled from a one-line editor selection; files are saved before searching and the open file reloads after a replace.
- [x] The MCP `search` tool now uses the same engine and gained `whole_word`, `regex`, `include`.
- [x] Tests: 11 in `search.rs` (case, whole word, regex, globs, binaries and `.moss` skipped, long lines, replace + undo, one file only, `$1`, no-op).
- Done notes: NOT tried in the running app (restart `tauri dev`). Not done: multi-line matches; a project-search history; replace preview per match.

## Phase 5 - moss-mcp hardening (from an agent's field report)
All four sub-phases done in one session at the user's request.
- [x] 5a Outside change detection: after each Agent tool call the files' size and modified time are stored in `.moss/mcp-seen.json`; at the start of the next call the first Project it opens is compared. Any difference is saved as a Version of kind `external` (author "Outside Moss"), logged in `audit.log` as `external_change`, and the answer carries `project_changed_outside {message, restore_point, modified, added, deleted}` (errors get the message appended). The first call only starts the record. Stale compile: when latexmk's console says "gave an error in previous invocation" the compile runs again with `-g`, `forced_rebuild: true`.
- [x] 5b `history_read_file(snapshot_id, path, start_line, end_line)` and `history_diff(a, b, path?)` (`a`/`b` = snapshot id, `working` or `empty`; unified diff, or the list of differing files).
- [x] 5c `compile` returns `boxes {overfull_hbox, overfull_vbox, underfull_hbox, underfull_vbox, worst:[{kind,file,line,pt,badness}]}` with the file worked out from the log's parentheses, `undefined_citations`, `undefined_refs`, and a `console_tail` without font paths (only the "Collected error summary" part when there are errors). `diagnostics` gets `boxes` and its `layout` entries now name the file.
- [x] 5d Smaller wins done: integrity report (`integrity {lines/bytes before and after, warnings}` on `edit_file`/`write_file`, `integrity_warnings` on `write_files`: only brace, `\begin`/`\end` and `$` problems the write introduced); `list_projects` adds `last_modified`; compile calls are logged in `audit.log`; Windows long paths (`\\?\` added in `safe_join` when a path passes 240 characters).
- Not done (and why): `overleaf_status.my_role` needs a network call but that tool is deliberately offline (`overleaf_projects` already returns `access_level`); `list_projects.note` has nowhere to be set yet; reads are not in `audit.log` and there is no client id (the MCP layer does not pass the tool name down; compile and Outside changes are logged instead).
- Done notes: 183 moss-core + 34 moss-mcp tests pass. The new tools show in Settings > MCP after restarting the app. Detection also fires for the user's own typing in the app (it cannot tell the app from another program), which is why the Version is called "Outside Moss".

## Phase 6 - Visual editor must not rewrite files lossily (found 2026-10-28)
An agent's report showed a file "corrupted" by a non-MCP writer (comments dropped, paragraphs joined, `\cite{a,b}` -> `[a,b]`, `\chapter`/`\label`/`\ref` flattened, `\section` and tables kept). The fingerprint matches `ui/lib/latex-visual.ts` (`stripComment`, `\cite` -> `[..]`, headings only map to section levels 1-3) driven by `latex-visual-editor.tsx` `onUpdate` -> `replaceDocumentBody`, which rewrites the whole body on any edit. The app's own saves are not in the MCP `audit.log`, so Moss never noticed.
- [x] Lossless round trip. `ui/lib/latex-visual.ts` rewritten: the body is cut into blocks that each keep their exact source and the whitespace after them (`parseVisualDocument`, `serializeVisualDocument`). Only plain words and `$inline maths$` become editable text; comments, `\cite`, `\ref`, `\textbf`, `\chapter`, `\section*`, equations, every other environment and anything unknown stay raw blocks. An untouched block is written back from its source, so a no-op round trip is byte-identical (CRLF, files with no `\begin{document}`, odd blank lines too). Text typed in the visual editor has LaTeX special characters escaped. `latex-visual-editor.tsx` uses the new API and skips a reload of LaTeX it wrote itself.
- [x] Restore point: `moss-app.tsx` `onVisualEdit` saves the file, then `api.historySave("Before visual editing of <path>")` once per file per session, before the first visual-editor edit (`EditorPane` got `onVisualChange`).
- [x] Tests: `ui/scripts/test-latex-visual.mts`, run with `npm run test:visual` in `ui/` (10 pass). `scripts` excluded from `tsconfig.json`.
- Done notes: `tsc` and eslint clean. NOT tried in the running app. Trade-offs: paragraphs that contain `\cite`, `\ref`, `\textbf` and the like now show as raw LaTeX blocks (safe, but less "visual"); equations in `equation`/`align` are raw, only `\[ ... \]` is an editable formula. An edited block can add one blank line next to its neighbour so text never merges. Follow-ups: inline raw atoms so a paragraph with `\cite` stays editable; log the app's own writes in the audit log (ties into 5a/5d).

## Phase 6b - UI refinement pass (2026-10-29, done, not seen running)
Sidebar (row buttons, last edited, filter, collapsed Archive, icon-only footer), project header (breadcrumb, Sync button group, tooltips via `components/moss/tip.tsx`), file tree and outline restyle, editor tabs with scroll arrows, Overleaf-style toolbar for Code and Visual (`editor-toolbar.tsx`), Find removed from the PDF preview, tooltips on its toolbar. Only `tsc`/eslint/cargo were run. The user will describe the "inside bar" (nested items) next.

## Phase 7 - Logo and brand (done 2026-10-30; plan: `docs/plans/m4-shipping-onboarding-brand.md` section 1)
- [x] Option B (typeset page) is the logo: `assets/brand/moss-logo.svg` (replaced the old mascot file; `moss-logo-b-page.svg` is the same artwork), `moss-mark-mono.svg` (one colour, `currentColor`), `moss-mark-small.svg` (flat, blue tile, for 16-48 px), `moss-wordmark.svg`, `moss-social.svg` + `moss-social.png` (1280x640, for the repo's social preview).
- [x] Used in the app: sidebar, welcome screen, setup banner use the full logo; favicon (`ui/app/icon.svg`) and the 16/24/32/48 px icon sizes use the flat mark, because the full artwork turns to a grey smudge below about 40 px (checked at 16 and 24 px).
- [x] App icons: `npx tauri icon` from the full logo, then `icon.ico` and `32x32.png` rebuilt with the flat mark for the small sizes.
- Left for the owner to decide: `moss-logo-a-mascot.svg` (option A, unused) and the `android/` and `ios/` folders `tauri icon` created in `src-tauri/icons` (unused by a desktop app). Neither is deleted.
- The wordmark and social image set the name as live text in Segoe UI (the app font), not as outlines; the PNG is the fixed version.

## Phase 8 - Setup core (`moss-core::setup`) (done 2026-10-29)
- [x] `check`, hash-pinned `download` (resume, cancel, mismatch deletes), `install_miktex` (`--unattended --private`, then AutoInstall on), `install_perl` (portable zip, staged then swapped in), `warm_packages`, `self_test`; Tauri commands `setup_check`, `setup_install`, `setup_cancel`, `setup_self_test` and a `setup-progress` event (`src-tauri/src/setup.rs`); `tex::moss_perl_bin` so detection finds the portable Perl. 8 tests (local-HTTP server: success, hash mismatch, cancel then resume, insecure URL refused; zip-slip; self test on this PC's MiKTeX). ADR 0006.
- URLs and hashes were checked on 2026-10-29 (MiKTeX basic 25.12 from miktex.org, Strawberry Perl 5.42.3.1 portable from GitHub; both answered 200 with the pinned sizes). **Not run:** the real MiKTeX install and Perl unpack (needs a clean machine: Windows Sandbox, Phase 9). Strawberry's zip is 305 MB; no smaller Perl found.
- Known: the `miktex packages` commands for `warm_packages` were only tried on already-installed packages; an unreachable repository is silently skipped. `cargo clippy -D warnings` still fails on an older `compile.rs:465` collapsible-if, not touched.

## Phase 9 - First-run onboarding UI (code done 2026-10-29, not seen running)
- [x] `setup-dialog.tsx` (check this PC, one-click install with per-part progress and Cancel, test compile, done with optional Connect Overleaf / Connect an agent, "Open the Moss tour"); `onboarded` in `settings.json` (opens once; closing counts as "not now"); the Compile button becomes "Set up LaTeX" when TeX is missing; Settings > Compiling has "Set up LaTeX..."; the welcome screen links to it. "Moss tour" Template (`tour`, with maths, figure, table, citation, cross-references; a test compiles it and checks no unresolved citations).
- [ ] **Not done:** a real run of the dialog and of the install on a clean Windows Sandbox (no TeX): confirm no popups appear, XeLaTeX/LuaLaTeX/biber compile, and the first compile with auto-install works. Windows Sandbox needs Windows Pro features turned on (**user**).
- [x] Setup dialog has a banner (aurora under a dark veil, logo, title); the welcome screen uses the same aurora.
- [ ] Welcome screen, sidebar rows and the dialog have only been type-checked and linted, never looked at.

## Phase 10 - Installer and updates (code done 2026-10-30)
- [x] NSIS per-user installer (`bundle.targets: ["nsis"]`, `installMode: currentUser`, WebView2 bootstrapper on demand, product exe named `Moss.exe`). `moss-mcp` is a sidecar (`externalBin`), so it installs beside `Moss.exe` where `agents.rs` looks for it; `scripts/prepare-sidecar.mjs` builds it (debug for `tauri dev`, `--release` for the installer) into `src-tauri/binaries/` (git-ignored) and runs before dev and build. A bare `cargo check -p moss-app` needs that folder to exist: run `node scripts/prepare-sidecar.mjs` once.
- [x] One version in four places (`Cargo.toml`, `package.json`, `ui/package.json`, `tauri.conf.json`): `node scripts/bump-version.mjs <version>` sets them, `--check [tag]` verifies (for CI).
- [x] In-app updates with `tauri-plugin-updater` (ADR 0007): signing key at `~/.tauri/moss-updater.key` (no password), public key and the `updater`-tag endpoint in `tauri.conf.json`; commands `update_check` and `update_install` (progress event, then restart); sidebar footer arrow with a popover; Settings > Updates (version, check now, install, errors). The earlier GitHub-API checker was dropped in favour of this.
- [ ] **User: back up `~/.tauri/moss-updater.key`** (losing it strands every installed Moss) and add it as the GitHub secret `TAURI_SIGNING_PRIVATE_KEY` (Phase 11).
- [ ] Not exercised: an update from one real build to a newer one (needs two published builds; do it with the first two betas). Code signing: unsigned betas, SignPath Foundation application in parallel (decided).

## Phase 11 - GitHub release
- [ ] **User**: how to handle the existing old `moss` repo (see plan), visibility, yes to the first commit. Then LICENSE, README, CONTRIBUTING, SECURITY, CHANGELOG, templates; `ci.yml`, `release.yml`; rename `master` to `main`; draft `v0.1.0-beta.1` pre-release. Full checklist in the plan file.

## Plan 2: sync freshness and layout tools (plan: `docs/plans/m5-agent-feedback-and-sync-freshness.md`)
- [ ] Phase 12 - Sync freshness: reproduce the missed collaborator edit (ask the user first), live cross-check via the socket, never-silent review fields, tests.
- [ ] Phase 13 - `compile` page count and delta; `page_text` with `fill_ratio`; `fit`.
- [ ] Phase 14 - `figures` report and missing-figure list; bib lint in `references`.
- [ ] Phase 15 - `measure`.
- [ ] Phase 16 - `render_diagram` (Graphviz, Mermaid, PlantUML, TikZ) with Approval + ADR; placeholder figures.
- [ ] Phase 17 - UI: Problems panel for missing figures and bib lint; review shows the new sync fields.

## Later: other platforms
- macOS and Linux are not supported. Windows-specific parts: MiKTeX/TeX Live paths and per-user install (`tex.rs`, `setup.rs`), `taskkill` and `CREATE_NO_WINDOW` process handling, the Windows credential store (`keyring` `windows-native`), the NSIS installer. A Mac port would need each of those redone, plus signing and notarisation. Not planned for the first releases.

## Other open items
- `ui/components/moss/agents-dialog.tsx` is unused; ask before deleting.
- Live Overleaf check on `test-moss` needs the user's go-ahead.
- Restart the app to look at Settings > MCP; try `moss-mcp` from a real client.
- Nothing is committed; ask first.
