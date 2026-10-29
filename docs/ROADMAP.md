# Moss roadmap

Order matters: prove the risky part (Overleaf sync) first.

- [ ] **M0 Feasibility spike**: code complete, **not yet verified against a real Overleaf account** (the owner must sign in).
  - [x] Overleaf client in `moss-core` (session, project list, zip download, file tree via socket.io, upload, delete), 8 offline tests.
  - [x] Sign-in three ways: Overleaf sign-in window (session cookie read from the window), Moss Connector Chrome extension (`extensions/chrome`, one-time code over 127.0.0.1), paste cookie. Session kept in Windows Credential Manager.
  - [x] Import from Overleaf: Pull creates an Overleaf-tagged Project, saves the Base snapshot, deletes the temporary zip.
  - [x] "Test write": uploads a tiny file to a chosen project, confirms it, deletes it, confirms it is gone.
  - [ ] Run it live: sign in, list projects, Import, Test write on a throwaway project. Fix whatever Overleaf's real responses show.
- [x] **M1 Core and local compile**
  - [x] Rust workspace (`moss-core`, `moss-mcp`, `src-tauri`) and static-export Next.js + shadcn UI in `ui/`; all build.
  - [x] TeX detection (custom path, PATH, MiKTeX/TeX Live folders, Perl) and engine auto-pick. MiKTeX installs missing packages silently (`--enable-installer`), so no popups.
  - [x] `latexmk` compile with persistent Build cache; subfolder `\include`, BibTeX, and errors-still-give-a-PDF covered by tests.
  - [x] Workspace and Project folders with `.moss/` metadata; Origin tag (Local / Overleaf) on every Project.
  - [x] Import a local zip or folder as a new Local Project (zip-slip safe, Windows-name safe, LF-normalized, Ignore list applied, Root file auto-detected).
  - [x] Project list with Origin tags, New / Import zip / Import folder, file tree, clickable diagnostics, Needs attention dialog.
  - [x] Run the real sample report through the core (18 files, cold 8.7 s, warm 0.69 s, PDF despite the template's own errors).
  - [x] Monaco editor bundled locally (`ui/scripts/copy-vendor.mjs` copies it to `public/vendor`, no CDN), LaTeX/BibTeX highlighting, T3 theme, live lint underlines, Ctrl+Enter compiles.
  - [x] Visual (Tiptap) editor and equation editor (MathLive + KaTeX) ported from the old Moss; Equation panel loads, inserts and replaces the equation at the cursor.
  - [x] PDF.js preview (lazy pages, zoom, fit width, scroll kept across compiles) with SyncTeX both ways through MiKTeX's `synctex`: "Show in PDF" (Ctrl+Shift+J) and double-click in the PDF.
  - [x] File tree like Overleaf's: new file, new folder, add files from this PC, rename, delete (each takes a History snapshot, so Undo works), plus a File outline of the open file's sections. Collapsible project sidebar, one "Add project" palette, and Moss's own prompt/confirm dialogs instead of browser pop-ups.
  - [x] Project sidebar actions: a per-project menu to Archive, Disconnect from Overleaf, or Delete (the delete modal makes you type the project's name; the folder and its History go, the Overleaf copy stays), an Archived list at the bottom of the sidebar, and a dithered contour-map header in the theme colour.
  - [x] Settings: Projects folder (applies after restart), custom TeX folder, per project: main file, engine, shell-escape, Trusted project. Shared with `moss-mcp` through `%APPDATA%\Moss\settings.json`.
- [ ] **M2 Sync and versions**: built and tested against a folder standing in for Overleaf; **not yet run against a live Overleaf account**. Plan and the 30-row edge-case matrix: `docs/plans/m2-sync-and-versions.md`; decision: ADR 0005.
  - [x] Three-way merge (`textdiff`, via `similar`) with structured conflict regions; line stats.
  - [x] History becomes a version log: author, kind, short id, line counts; Save version, compare any two revisions, file at a revision, restore a version or one file, locked index, undo steps over event markers.
  - [x] Sync engine in `moss-core::sync`: `Remote` trait (`OverleafRemote`, `FolderRemote`, `MemoryRemote`), per-file decision table, rename detection, Review then Apply with a staleness token, per-file Base settling, partial-failure and auth-expiry handling, mass-deletion guard, empty-remote guard, Needs attention (Windows names, case twins). 47 sync tests + 5 link tests.
  - [x] Link a Local Project to a new Overleaf project (zip upload), an existing one (no Base: nothing deleted, differences asked), or a Practice copy; unlink; Practice project from sample files.
  - [x] Overleaf client: create folder, rename, move, create project from zip, CSRF reuse, retry on 429/5xx. The live "Test write" now also covers folder, rename and move.
  - [x] App: **History tab** (log with graph dots, SHA, subject, author, date, changes; Working copy row; Monaco side-by-side diffs with collapsed unchanged lines; restore version / file; discard; Save version), Sync menu (Pull, Push, Sync, Unlink, Practice folder), Review dialog with per-file diffs and untick, Conflict screen (per hunk Mine/Theirs/Both/Edit, whole-file, by hand), result screen, "Connect to Overleaf" dialog, Practice project in Add project.
  - [x] Tree actions and Agent writes now record a Version after they run (author You / Agent).
  - [x] MCP: `history_save`, `history_show`, `history_restore_file`, `overleaf_status`, `overleaf_review`, `overleaf_sync` (push and sync need Approval, conflicts returned for the agent to resolve).
  - [x] MCP: `overleaf_pull`, `overleaf_push`, `overleaf_conflicts` (check before syncing, region by region) and `overleaf_resolve` (one answer per conflicting file or region; mistyped choices and stray paths are refused). Pull needs no Approval; push and sync keep it unless the Project is trusted.
  - [x] One Apply at a time per Project (file lock shared by the app and `moss-mcp`); `overleaf_link` checks the id against the account; clippy, `tsc` and eslint clean.
  - [x] MCP: `overleaf_projects`, `overleaf_history` (Overleaf's own record, read-only), `overleaf_link` (existing, or new, which needs Approval) and `overleaf_unlink` (needs Approval); `overleaf_status` also reports sign-in.
  - [x] History tab has an **On Overleaf** view: Overleaf's own change history (who edited, created, deleted or renamed what, labels), read-only. Parsing is tested; the `/updates` endpoint is unverified until a live run.
  - [ ] Live check: sign in, import, **Test write** (now with folder/rename/move) on a throwaway project, then Push and a conflict on a throwaway project. Fix what Overleaf's real responses show.
  - [ ] Later: hash cache so the 15 s change check is cheap on very large projects; Review progress for slow uploads; per-hunk "Both" ordering options.
- [ ] **M3 Agents**: mostly built, needs a real client session.
  - [x] History snapshots in `moss-core` (`.moss/history/`, content-addressed): capture before every change, undo, redo, restore (itself undoable), changes since a snapshot; 7 tests.
  - [x] Approvals in `moss-core` (folder of JSON files, five-minute timeout) and an Approve/Deny prompt in the app; audit log at `.moss/audit.log`.
  - [x] `moss-mcp` (rmcp 3.7, stdio, `--workspace` or `MOSS_WORKSPACE`): list/create/import projects, project info, list/read/write/edit/delete/move files, search, compile with diagnostics, compile options, shell-escape, history list/changes, undo, redo, restore. Deleting and enabling shell-escape wait for an Approval unless the Project is trusted. 8 tests plus a stdio protocol smoke test.
  - [x] App: undo/redo buttons, copy-paste config for agents (now on the MCP page of Settings), reload of files an agent changed.
  - [ ] Try it from a real client (Claude Code / Claude Desktop) and fix what shows up.
  - [x] Trusted project switch in Settings.
  - [x] History list/restore screen (the History tab) and Overleaf tools (see M2).
  - [x] MCP, seeing and checking the document: `view_pages` (Page images of one page, a range, a list, `last` or `all`, as separate images or one contact sheet; compiles first when the PDF is missing or stale), `pdf_info`, `view_image`, `source_to_pdf` and `pdf_to_source` (SyncTeX), `diagnostics` (last log, with overfull/underfull boxes), `clean_build`, `outline` (sections across `\input`/`\include`), `references` (undefined `\ref`/`\cite`, duplicates, unused labels and `.bib` entries), `list_templates`, and `create_project` with a Template. Pages are drawn by MiKTeX's Poppler (`pdftoppm`); nothing new is bundled.
  - [x] Templates in `moss-core`: article, paper (IEEEtran), report, thesis, beamer, letter, cv, unicode (XeLaTeX).
  - [x] Template and page-image check across all formats through the real MCP protocol (all 8 templates compile with 0 errors; page images, ranges, SyncTeX and diagnostics checked).
  - [x] MCP reads and writes made cheaper: `read_file` takes `paths` (many files) or a line range, stops at 60,000 characters and says where to continue; `edit_file` takes several `edits`, `write_files` writes up to 50 files, `delete_file` takes `paths` (one Approval); each call is one restore point, and a no-op takes none.
  - [x] Every tool has a title and read-only/destructive/online hints, checked by a test; `moss-mcp --list-tools` prints them.
  - [x] Settings is a left-nav page (General, This project, MCP) replacing the "Connect an agent" dialog. MCP page: server status (ready, not found, not responding), program and folder, TeX and Poppler checks, which AI programs (Claude Code, Claude Desktop, Cursor, Codex) are set up and whether they point at this copy, recent agent activity, and every tool with an on/off switch (`disabled_tools` in `settings.json`; `moss-mcp` hides them from agents at its next start).
  - [x] Overleaf realtime step (Push's file-tree read) keeps the handshake's `GCLB` load-balancer cookie and retries transient 502/503/504; before, most attempts failed with `502 Bad Gateway`.
  - [x] Project search and replace (`moss_core::search`, Ctrl+Shift+F): plain, case, whole word, regex, file filters; replace in one file or everywhere with one restore point; the MCP `search` tool uses the same engine.
  - [x] MCP hardening from an agent's field report: Outside change detection (`mcp-seen.json`, Version kind "Outside Moss"), forced rebuild when latexmk replays an old failure, `history_read_file` and `history_diff`, structured `boxes`/`undefined_citations`/`undefined_refs` in `compile`, trimmed `console_tail`, brace/environment/`$` balance report on writes, `last_modified` in `list_projects`, `\\?\` long paths, compile and Outside changes in the audit log.
  - [x] Compile lock per Build cache (`compile.lock`) so two compiles never write one PDF.
- [ ] **M4 Shipping**: plan in `docs/plans/m4-shipping-onboarding-brand.md` (phases 7-11 in `docs/PHASES.md`).
  - [x] Logo, small mark, mono mark, wordmark, social image (`assets/brand/`); app icons.
  - [ ] First-run setup: one-click MiKTeX + Perl, package warm-up, "Moss tour" test project. Backend (`moss_core::setup`, ADR 0006), dialog and tour Template written; clean-machine run still to do. Windows only for now.
  - [x] Per-user NSIS installer with `moss-mcp.exe`, in-app updates (signed), version script, Settings > Updates (ADR 0007). Update-to-newer-build still to be exercised with real releases.
  - [ ] GitHub: LICENSE/README/CI, draft `v0.1.0-beta.1` pre-release.
  - [x] "Connect an agent" screen (Settings > MCP).

## Reference numbers
moss-core test fixture (multi-file, subfolder include, BibTeX): cold 14.7 s, warm 1.1 s.
Sample report (16 files, pdfLaTeX, BibTeX) on MiKTeX: cold 23.9 s, no-change rebuild 0.7 s, one-chapter edit 2.7 s.
