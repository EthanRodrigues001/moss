# M5 plan: sync freshness and layout tools for agents

Source: a field report from an agent writing an 85-page, multi-file report in Moss, plus the user's own sync bug. Domain words: `GLOSSARY.md`. Status is ticked in `docs/ROADMAP.md` and `docs/PHASES.md`.

Principle: answer layout questions as **text and numbers**, not images. Everything below is a tool in `moss-core` first, then an MCP tool, then (where useful) UI.

## A. Sync bug first: a collaborator's edit never arrived (Pull and Sync both said nothing)

Reported: a friend edited the Overleaf project; Moss showed no change on Pull or Sync, and the local copy stayed old. Possible causes, in the order to check:

1. **Stale remote read.** `OverleafRemote::fetch` reads one zip (`/project/{id}/download/zip`, `overleaf/remote.rs`). Edits made in the live editor sit in Overleaf's realtime store until it flushes them, so the zip can lag a few minutes behind what the friend sees. Check against the live editor, not the zip.
2. **Friend's change is in a file Moss skips** (binary over the size cap, ignored name, `skipped` list from `FileSet::from_zip`). Then nothing is reported because nothing is counted.
3. **Base already equals the remote** because an earlier Sync adopted content (decision table row "changed, L == R: adopt") and the friend's edit landed in the same window.
4. **Wrong project or renamed folder**: the link points at a different Overleaf id than the one the friend edits (copy of the project, or a re-import).
5. **Cached 15 s change check** (`token_of`) reusing an old token.

Work:
- [ ] **Reproduce on a throwaway Overleaf project** (needs the user's go-ahead): edit in the browser, then immediately and after 1, 3, 10 minutes call `overleaf_review`; record when the zip catches up. This decides between cause 1 and the rest.
- [ ] **Live cross-check**: the realtime socket (`overleaf/socket.rs`) already joins the project. After the zip, for every text doc compare the socket's live content (`joinDoc`) with the zip; where they differ, use the live text and mark the file "newer than the download". Cost is one `joinDoc` per doc, so do it only when `overleaf_history` (or the project `version` from `joinProject`) says the project changed since the last Sync.
- [ ] **Never silent**: `Preview` gains `remote_checked_at`, `remote_files`, `remote_skipped` (with reason per file) and `remote_last_edit` (from Overleaf history). If history shows an edit newer than anything the zip contained, the review says "Overleaf has newer edits than the download; try again in a minute" instead of "up to date".
- [ ] **"Why is nothing changing?"** link in the review: lists the Base time, the remote fetch time, counts, and the skipped files.
- [ ] **Tests** with a fake `Remote` that returns an old snapshot while history reports a newer edit (must warn, must not claim "up to date"); a skipped-file test; a stale-token test. Add the edge-case row to the table in `docs/plans/m2-sync-and-versions.md`.
- [ ] Same checks reachable from MCP: `overleaf_status` and `overleaf_review` return the new fields.

## B. Layout and measurement tools (MCP, text first)

| # | Tool | What it returns | Built on |
|---|---|---|---|
| B1 | `compile` extras | `pages`, `pages_before`, `pages_delta`, list of pages whose content moved (by first heading per page) | last build's `.log`/PDF, kept in `.moss/` |
| B2 | `page_text` | per-page extracted text for a page or range, plus `fill_ratio` (0 to 1, from the lowest ink line) and `overflowed_from` (page where a heading/environment began and continued) | `pdftotext -bbox-layout` (MiKTeX ships it next to `pdftoppm`) |
| B3 | `figures` | every `\includegraphics`: file, source line, resolves yes/no, pixel size, printed width/height in mm, effective DPI, exceeds text block yes/no | source scan + `\the\textwidth` probe + image header read |
| B4 | `measure` | width, height, depth in pt and mm of a snippet in the document's real font and size; optional `max_width` returns "fits" or "over by X mm" | throwaway `\settowidth` job reusing the project preamble, run through the normal compile guard |
| B5 | `fit` | for a page: remaining vertical space in mm; for a fixed-height environment (`titlepage`): over/under by X mm | B2 geometry plus B4 |
| B6 | `references` lint | missing required fields per bibliography style (`ieeetr`, `plain`, `IEEEtran`, biblatex defaults), missing DOI, duplicate keys, cited but absent | existing `references` parser plus a per-style field table |
| B7 | placeholder figures | project option `placeholder_missing_figures`; Moss injects an `\IfFileExists` wrapper through `\AtBeginDocument` so unresolved images draw a labelled box | compile-time preamble injection (build copy only, never the user's source) |

Notes:
- B3's missing-file list also feeds the Problems panel in the UI, so a person sees it too.
- B4 and B5 compile a scratch document in `.moss/build/measure/`, never in the Project, with the same `compile.lock` rules.
- B2 must keep the per-page byte cap that `view_pages` has, and say where to continue.

## C. Diagram rendering

| # | Tool | Notes |
|---|---|---|
| C1 | `render_diagram` | source `.mmd`, `.dot`, `.puml`, `.tikz` in the Project, output PDF or PNG next to it. Uses the user's own installs (Graphviz `dot`, Java + PlantUML jar, `mmdc`) found like `tex::detect` finds TeX. Anything that runs an external program goes through the same Approval gate as shell-escape, remembered per Project, skipped when the Project is trusted. |
| C2 | Setup check | Settings > Setup lists which renderers exist; missing ones are named, never auto-installed in this milestone. |

TikZ needs no new program: render it as a standalone `.tex` through the existing compile path. Write an ADR (0006 or next free number) because "Moss runs external renderers on project files" is a security-relevant, hard-to-reverse decision.

## D. Host interaction (note only)

Claude Code warns "file changed since you last read it" after `edit_file`. Moss cannot mark a write as known to the harness. Mitigation: `edit_file` and `write_file` already echo context lines; add the new content hash to the result so an agent can skip a re-read. No further work planned.

## Build order

| Step | Content | Done when |
|---|---|---|
| 1 | A: reproduce, then live cross-check and never-silent review | the friend scenario is a passing test; live check on a throwaway project |
| 2 | B1 | page count and delta in `compile` |
| 3 | B2, B5 | `page_text` and `fill_ratio` on the sample report |
| 4 | B3 | missing figures found on the 16-figure report |
| 5 | B6 | the two DOI-less entries are reported |
| 6 | B4 | the declaration title width matches a render within 0.5 mm |
| 7 | C1, C2, ADR | a `.dot` and a `.mmd` render to PNG inside a Project, with Approval |
| 8 | B7 | placeholder boxes appear only in the build copy |
| 9 | UI | Problems panel shows missing figures and bib lint; review shows the new sync fields |

Each step: tests in `moss-core`, one MCP tool test, `--list-tools` hints (read-only vs destructive), a ROADMAP tick, GLOSSARY entries for new terms (fill ratio, placeholder figure, renderer).

## Questions for the user

- Step 1 needs a throwaway Overleaf project and a second account (or the browser editor) to reproduce the friend's edit. OK to run the live check?
- C1: which renderers matter first? Suggested order: Graphviz, Mermaid, PlantUML.
