# M2 plan: Sync, History tab, Conflicts

Decision record: ADR 0005. Domain words: `GLOSSARY.md`. Status is ticked in `docs/ROADMAP.md`.

## What the user gets

1. **Pull, Push, Sync** buttons on an Overleaf Project. Each opens a *review* first (what will change, per file, with a diff), then applies.
2. **Conflicts** get their own step: per overlapping hunk choose Mine, Theirs or Both, or edit the result; whole-file choice for binaries and modify-versus-delete.
3. A **History tab** next to the editor: a git-style log (graph dot, short id, subject, author, date, changes), the files of a selected version, and a side-by-side diff with collapsed unchanged regions. A top row shows the **working copy** (changes not yet saved as a version).
4. **Save version** (with a message), **Restore version**, **Restore one file from a version**. Every restore is itself undoable.
5. **Practice project**: a folder on this PC that behaves like an Overleaf project, so Sync can be tried and tested with no account and no risk.

## Build order

| Step | Where | Done when |
|---|---|---|
| 1 | `moss-core::textdiff`, `merge3` | line diff, stats, three-way merge with structured conflicts; unit tests |
| 2 | `moss-core::history` | versions carry author, kind, short id, stats; compare any two; file contents at a version; restore one file; save version |
| 3 | `moss-core::sync` | `FileSet`, `plan`, `Remote` trait, `FolderRemote`, `preview`, `apply`; the edge-case matrix below as tests |
| 4 | `moss-core::overleaf` | `OverleafRemote`: tree ids, create folder, upload, delete, rename/move; retry on 429/5xx |
| 5 | `src-tauri`, `moss-mcp` | commands and MCP tools (`sync_status`, `sync_preview`, `pull`, `push`, `sync`, history compare/restore) with Approvals on Push |
| 6 | `ui` | History tab, review and Conflict dialogs, Pull/Push/Sync buttons, Practice project |
| 7 | live | M0 live test, then Push on a throwaway Overleaf project |

## The per-file decision table

`B` = Base, `L` = local, `R` = remote. "Same" compares content (text is LF-normalized first).

| B | L vs B | R vs B | Result |
|---|---|---|---|
| any | same | same | nothing |
| present | changed | same | **Push** the change (upload / delete) |
| present | same | changed | **Pull** the change (write / delete) |
| present | changed | changed, L == R | adopt (no I/O), Base updated |
| present | changed | changed, L != R, both text | **Merge** (diff3); clean: write merged locally and push it; overlapping: **Conflict** |
| present | modified | deleted | **Conflict** (keep mine / accept delete) |
| present | deleted | modified | **Conflict** (accept delete / keep theirs) |
| present | deleted | deleted | adopt |
| present | binary changed | binary changed, differ | **Conflict** (whole file) |
| none | added | none | **Push** |
| none | none | added | **Pull** |
| none | added | added, L == R | adopt |
| none | added | added, differ | **Conflict** (two-way, no common ancestor; text shows both versions) |

Rename detection runs before the table: a delete and an add with the same content on one side, in the same Sync, is one **rename**. Pushed as Overleaf's rename/move (keeps its history); applied locally as a move. A rename on one side plus an edit on the other is treated as delete plus add and so becomes a Conflict the user can resolve, never a silent loss.

## Edge cases and the test that proves each

Every row is a test in `moss-core` (sync tests use `FolderRemote`; some wrap it in a `FaultyRemote`).

| # | Case | Expected |
|---|---|---|
| 1 | First Sync after Pull, nothing changed | plan empty, no network writes |
| 2 | Edit different files on each side | both applied, Base updated per file |
| 3 | Edit different lines of one file on both sides | merged cleanly, merged text lands on both sides |
| 4 | Edit the same line on both sides | Conflict with Mine/Theirs/Base text; nothing written until resolved |
| 5 | Both sides made the identical edit | adopted, no Conflict |
| 6 | Modify local, delete remote (and the reverse) | Conflict, either resolution works |
| 7 | Both added a file with the same name, differing | Conflict; identical content adopted |
| 8 | Binary changed on both sides | whole-file Conflict |
| 9 | Remote changed between preview and apply | apply refuses with `RemoteChanged`, nothing written |
| 10 | Push fails on one file (rejected, network) | other files still pushed; only successful files advance Base; failure listed; re-run finishes |
| 11 | Auth expires mid-apply | stop immediately, local files already applied stay, Base consistent |
| 12 | Remote returns an empty project, Base had files | refused as a probable error (not "delete everything") |
| 13 | Many deletions (>=5 and >30%) | needs `allow_mass_delete` |
| 14 | Remote file named `aux.tex`, `a:b.tex`, or two names differing by case | Needs attention; not pulled, not deleted, not counted as local deletion |
| 15 | CRLF edits in Notepad | normalized to LF before comparing and pushing; no phantom change |
| 16 | Ignored files (`.aux`, `.moss/`, `.git`) | never in a plan, never pushed |
| 17 | File in a new nested folder | remote folders created first; local empty folders removed after a pull deletes the last file |
| 18 | Remote rename | applied as a move locally; local edits to the old name become a Conflict |
| 19 | Local rename | `rename` on the remote (not delete+add) |
| 20 | Text file replaced by a same-named binary (type change) | upload; Overleaf swaps the file's type itself (to confirm live) |
| 21 | Local edits while the review is open | apply re-reads local files; if they changed since the preview, refuse with `LocalChanged` |
| 22 | Pull-only with unpushed local edits | local-only changes untouched and stay "ahead"; merged files do not advance Base |
| 23 | Push blocked for a file Overleaf also changed | skipped with "Pull first"; others pushed |
| 24 | Root file deleted by Pull | warning shown; Root file setting untouched |
| 25 | Non-UTF-8 "text" file | treated as binary (whole-file Conflict only) |
| 26 | Resolution text still contains `<<<<<<<` | warned, requires confirmation |
| 27 | Apply crashes mid-way | History snapshot "before sync" exists; one Undo restores |
| 28 | Large file (> 2 MB text) in a diff | diff skipped with a size note; sync still handles it as a whole file |
| 29 | Practice and real remotes | same code path, selected by the link's project id prefix |
| 30 | `sync.json` / Base missing on a linked Project | treated as first Sync: nothing is deleted, differing files become Conflicts |

## History model

- A version is one **History snapshot** (ADR-free: this extends `history.rs`). New fields, all `serde(default)` so older indexes load: `kind` (`auto`, `manual`, `agent`, `pull`, `push`, `merge`, `restore`), `author` (`You`, `Agent`, `Overleaf`, `Moss`), `sha` (first 7 hex of a hash of the file set and parent), `stats` (files changed, lines added and removed against the previous version).
- Versions are the state *after* a change. Before an Agent or file-tree change Moss saves "Your changes" if you typed since the last version; after it, one version describing the change.
- Compare takes two revisions (a version id or the working copy) and returns per-file changes; file contents at a revision feed the Monaco diff editor.
- Capped at 200 versions; oldest dropped with their unshared blobs.

## UI shape

- Header tabs: **Editor** and **History**. The tab bar sits in the project header; the sidebar and the Compile button stay.
- History tab: toolbar (Save version, Pull, Push, Sync, Origin status), log table, then changed files and the diff for the selected version.
- Review dialog (Pull/Push/Sync): list grouped Pull / Push / Merge / Conflict / Needs attention, click a file for its diff; Conflicts step next; Apply last.
- Practice project lives in the Overleaf dialog.

## Status

Steps 1 to 6 are built; step 7 (live) is waiting on a real account. Every row above has a test in `crates/moss-core/src/sync/tests.rs` except 20 (Overleaf-side behaviour) and 28 (the diff viewer shows a size note; covered in `history.rs`). Known cost: the header's change count re-reads every file each 15 s; a hash cache fixes that if it shows on big projects.

## Out of scope for M2

Realtime co-editing with Overleaf users, Overleaf comments/track changes, Overleaf's own version labels, multi-remote, partial (per-file) Sync selection beyond the review's file checkboxes.
