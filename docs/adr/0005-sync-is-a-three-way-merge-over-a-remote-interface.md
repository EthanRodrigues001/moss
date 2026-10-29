# Sync is a three-way merge against the Base snapshot, behind a Remote interface

Overleaf has no public write API and no git for free-plan users (ADR 0002), so Moss cannot ask Overleaf to merge. Moss merges itself.

**Decision**

- A file's state is compared in three places: the **Base snapshot** (last agreed state, `.moss/base/`), the **local** files, and the **remote** files (a fresh zip from Overleaf, held in memory and never kept on disk). Per file: changed on one side only is applied to the other; changed on both sides the same way is adopted; changed on both sides differently is merged line by line (diff3) and, if lines overlap, becomes a **Conflict** the user resolves. Binary files and modify-versus-delete are whole-file Conflicts.
- Sync is **preview first, apply second**. The preview changes nothing. Apply fetches Overleaf again and refuses if it changed since the preview, so the user never confirms one plan and gets another.
- The **Base snapshot is updated per file** as each file is settled, never as one block at the end. A Push that stops half way (network, a file Overleaf rejects) leaves a consistent state, and the next Sync finishes the job.
- **Push never overwrites an Overleaf change it has not merged.** A file changed on Overleaf is pulled and merged first; Push skips it and says so.
- **One Apply at a time per Project.** The app and `moss-mcp` are separate processes, so Apply takes an operating-system file lock (`.moss/sync.lock`) and refuses, without waiting, when another holds it. The lock frees itself if a process dies. A Review needs no lock.
- Everything Moss does to the files (a Pull applying, a merge, a Conflict resolution, a restore) takes a **History snapshot** first, so it can be undone.
- All Overleaf access sits behind a small `Remote` trait (`fetch`, `upload`, `delete`, `rename`). The real `OverleafRemote` and a folder-backed `FolderRemote` implement it. Tests, the Practice project and agents-in-CI use `FolderRemote`; nothing in the sync engine knows an Overleaf URL.
- Guards: mass deletion (more than 30% of files and at least 5, or an empty remote against a non-empty Base) needs an explicit confirmation; names Windows cannot hold and case-only duplicates go to Needs attention and are left alone on both sides.

**Why it is hard to reverse**

The Base snapshot format, the per-file settling rule and the preview/apply split shape every Sync screen, every MCP tool and the on-disk metadata. Changing them later means migrating users' Base snapshots.

**Alternatives rejected**

- *Push replaces Overleaf with the local copy (and Pull the reverse)*: simple, but silently destroys the other side's edits, which is the exact failure users fear from Overleaf sync tools.
- *Use git on both sides*: Overleaf's git bridge is a paid feature, and a local git repo adds a second source of truth next to `.moss/history`.
- *Merge on Overleaf through its realtime editor*: needs the full operational-transform protocol and is far more fragile than the zip plus upload endpoints.

**Known limits**

- Overleaf's upload, folder and rename endpoints are unofficial and not yet verified against a live account (M0). The write test exercises upload and delete; creating folders and renaming join it before Push ships to users.
- A file open in someone's Overleaf editor while Moss uploads over it is overwritten without operational-transform merging. Pull before Push is the mitigation.
- Overleaf's own project history is shown read-only in the History tab ("On Overleaf"); it is never merged into Moss's own versions, and restoring from it is done by Pulling.
