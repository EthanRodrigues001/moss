// Browser-only stand-ins for the History and Sync commands, so the screens can be designed and
// checked without the desktop app. Nothing here ships into the Tauri build's behaviour.

import type {
  Applied,
  ApplyRequest,
  FileChange,
  FileView,
  Rev,
  SnapshotInfo,
  StagedFile,
  SyncPreview,
  SyncStatus,
  SyncMode,
  VersionDetail,
} from "./types";

const NOW = Math.floor(Date.now() / 1000);

const v = (
  id: number,
  sha: string,
  label: string,
  kind: SnapshotInfo["kind"],
  author: string,
  ago: number,
  stats: SnapshotInfo["stats"],
): SnapshotInfo => ({
  id,
  sha,
  label,
  kind,
  author,
  created_at: NOW - ago,
  file_count: 18,
  stats,
  current: false,
});

const versions: SnapshotInfo[] = [
  v(1, "7381a3a", "Initial project baseline", "manual", "You", 86400 * 5, { files: 18, added: 2431, removed: 0 }),
  v(2, "c40e91d", "Pulled from Overleaf: 3 files", "pull", "Overleaf", 86400 * 3, { files: 3, added: 64, removed: 12 }),
  v(3, "e8b6720", "edit_file: chapters/Chapter1Introduction.tex", "agent", "Agent", 86400 * 2, { files: 1, added: 9, removed: 3 }),
  v(4, "52fd0b1", "Your changes", "auto", "You", 3600 * 20, { files: 2, added: 21, removed: 8 }),
  v(5, "9a3c4e6", "Merged with Overleaf: 2 files, 1 conflict resolved", "merge", "Moss", 3600 * 5, { files: 2, added: 14, removed: 6 }),
  v(6, "a8f112b", "Pushed 2 files to Overleaf", "push", "You", 600, { files: 2, added: 35, removed: 14 }),
];
versions[5].current = true;

const CHAPTER = [
  "\\chapter{Introduction}",
  "\\label{chap:intro}",
  "",
  "Digital twins of industrial machines are becoming a standard tool for",
  "monitoring, planning and prediction. This report describes the design",
  "of a ledger-based material requirements planning (MRP II) dashboard.",
  "",
  "\\section{Motivation}",
  "Planners spend a large share of their week reconciling spreadsheets.",
  "A single source of truth removes that work and the errors it causes.",
  "",
  "\\section{Objectives}",
  "\\begin{itemize}",
  "  \\item Record every stock movement as an immutable ledger entry.",
  "  \\item Derive the plan from the ledger, never the other way round.",
  "  \\item Explain every number the dashboard shows.",
  "\\end{itemize}",
  "",
  "\\section{Scope}",
  "The first release covers purchasing, production orders and stock.",
  "Quality management and maintenance are out of scope.",
  "",
  "\\section{Method}",
  "We follow an iterative approach with a fortnightly review.",
  "Each iteration ends with a demonstration to the planners.",
  "",
  "\\section{Structure of this report}",
  "Chapter~\\ref{chap:design} presents the design and the ledger schema.",
  "Chapter~\\ref{chap:results} evaluates the dashboard against the goals.",
  "",
  "\\section{Related work}",
  "Several open-source ERP systems implement MRP II. None keeps a ledger.",
  "Commercial systems do, but their plans cannot be explained step by step.",
  "",
  "\\section{Summary}",
  "The next chapter introduces the data model.",
];

/** The chapter as it was in each version: a few lines differ so the diffs show real hunks. */
function chapterAt(version: number): string[] {
  const lines = [...CHAPTER];
  if (version >= 2) lines[4] = "of a ledger-based material requirements planning (MRP II) dashboard.";
  if (version >= 3) lines.splice(15, 0, "  \\item Keep the plan reproducible from any past date.");
  if (version >= 4) lines[9] = "A single source of truth removes that work and most of its errors.";
  if (version >= 5) lines[33] = "Several open-source ERP systems implement MRP II; none keeps a ledger.";
  if (version >= 6) lines[lines.length - 1] = "The next chapter introduces the data model and the ledger schema.";
  if (version >= 7) lines.push("", "% TODO: add a sentence about the pilot users.");
  return lines;
}

const SYNOPSIS = [
  "\\documentclass[12pt]{report}",
  "\\usepackage{graphicx}",
  "\\usepackage{amsmath}",
  "\\begin{document}",
  "\\input{chapters/Chapter1Introduction}",
  "\\end{document}",
];

function fileView(text: string | null): FileView {
  return text === null
    ? { exists: false, binary: false, size: 0, text: null, too_large: false }
    : { exists: true, binary: false, size: text.length, text, too_large: false };
}

function revNumber(rev: Rev): number {
  return rev.type === "empty" ? 0 : rev.type === "working" ? 7 : rev.id;
}

export function mockHistoryList(): SnapshotInfo[] {
  return structuredClone(versions);
}



const filesOf = (id: number): FileChange[] => {
  switch (id) {
    case 1:
      return [
        { path: "0Synopsis.tex", kind: "added", binary: false, added: 6, removed: 0 },
        { path: "chapters/Chapter1Introduction.tex", kind: "added", binary: false, added: 37, removed: 0 },
        { path: "fcritlogo.jpg", kind: "added", binary: true, added: 0, removed: 0 },
      ];
    case 6:
      return [
        { path: "chapters/Chapter1Introduction.tex", kind: "modified", binary: false, added: 1, removed: 1 },
        { path: "0Synopsis.tex", kind: "modified", binary: false, added: 2, removed: 0 },
      ];
    default:
      return [{ path: "chapters/Chapter1Introduction.tex", kind: "modified", binary: false, added: 2, removed: 1 }];
  }
};

export function mockHistoryVersion(id: number): VersionDetail {
  return {
    from: id <= 1 ? { type: "empty" } : { type: "snapshot", id: id - 1 },
    to: { type: "snapshot", id },
    files: filesOf(id),
  };
}

export function mockHistoryWorking(): VersionDetail {
  return {
    from: { type: "snapshot", id: 6 },
    to: { type: "working" },
    files: [{ path: "chapters/Chapter1Introduction.tex", kind: "modified", binary: false, added: 2, removed: 0 }],
  };
}

export function mockHistoryFile(rev: Rev, path: string): FileView {
  if (path.endsWith(".jpg")) return { exists: rev.type !== "empty", binary: true, size: 41000, text: null, too_large: false };
  const n = revNumber(rev);
  if (n === 0) return fileView(null);
  if (path === "0Synopsis.tex") return fileView(SYNOPSIS.join("\n") + "\n");
  if (path.startsWith("chapters/")) return fileView(chapterAt(n).join("\n") + "\n");
  return fileView(null);
}

// ----- Sync -----

let linked = true;
let ahead = [{ path: "chapters/Chapter1Introduction.tex", kind: "modified" as const }];

export function mockSyncStatus(project: string): SyncStatus {
  void project;
  return {
    link: linked
      ? { project_id: "abc123", name: "Main Project Report", last_synced: NOW - 600 }
      : null,
    practice: false,
    practice_folder: null,
    changes: linked ? ahead : [],
  };
}

export function mockSetLinked(next: boolean) {
  linked = next;
}

const THEIRS_CHAPTER = chapterAt(6).map((l) => l.replace("pilot", "pilot")).join("\n") + "\n";

export function mockSyncPreview(mode: SyncMode): SyncPreview {
  const items: SyncPreview["items"] = [
    { path: "chapters/Chapter1Introduction.tex", from: null, direction: "pull", op: "modify", applies: mode !== "push", note: mode === "push" ? "Overleaf has a newer version; Pull to get it." : null, binary: false, lines: { added: 3, removed: 1 } },
    { path: "0Synopsis.tex", from: null, direction: "push", op: "modify", applies: mode !== "pull", note: mode === "pull" ? "Stays on this PC (Pull does not send changes)." : null, binary: false, lines: { added: 2, removed: 0 } },
    { path: "0Synopsis.bib", from: null, direction: "both", op: "merge", applies: mode !== "push", note: mode === "push" ? "Overleaf also changed this file. Pull first." : null, binary: false, lines: { added: 4, removed: 1 } },
    { path: "fig/pipeline.png", from: null, direction: "pull", op: "add", applies: mode !== "push", note: null, binary: true, lines: { added: 0, removed: 0 } },
    { path: "chapters/Chapter2Design.tex", from: null, direction: "both", op: "conflict", applies: mode !== "push", note: mode === "push" ? "Overleaf also changed this file. Pull first." : null, binary: false, lines: { added: 0, removed: 0 } },
    { path: "chapters/Draft.tex", from: null, direction: "push", op: "delete", applies: mode !== "pull", note: null, binary: false, lines: { added: 0, removed: 9 } },
  ];
  return {
    token: "mock-token",
    mode,
    items,
    conflicts:
      mode === "push"
        ? []
        : [
            {
              path: "chapters/Chapter2Design.tex",
              kind: "both_modified",
              binary: false,
              mine_exists: true,
              theirs_exists: true,
              base: "\\chapter{Design}\nThe ledger has one table.\nEvery row is immutable.\n\\section{Schema}\nSee the appendix.\n",
              mine: "\\chapter{Design}\nThe ledger has three tables.\nEvery row is immutable.\n\\section{Schema}\nSee the appendix.\n",
              theirs: "\\chapter{Design}\nThe ledger is one append-only table.\nEvery row is immutable.\n\\section{Schema}\nSee Appendix~A.\n",
              regions: [
                { type: "stable", text: "\\chapter{Design}\n" },
                { type: "conflict", base: "The ledger has one table.\n", mine: "The ledger has three tables.\n", theirs: "The ledger is one append-only table.\n" },
                { type: "stable", text: "Every row is immutable.\n\\section{Schema}\n" },
                { type: "conflict", base: "See the appendix.\n", mine: "See the appendix.\n", theirs: "See Appendix~A.\n" },
              ],
            },
          ],
    needs_attention: [{ path: "aux.tex", reason: "`aux.tex` is a reserved Windows name" }],
    warnings: [],
    mass_delete: null,
    blocked: null,
    summary: {
      pull: mode === "push" ? 0 : 2,
      push: mode === "pull" ? 0 : 1,
      merge: mode === "push" ? 0 : 1,
      conflict: mode === "push" ? 0 : 1,
      delete_here: 0,
      delete_there: mode === "pull" ? 0 : 1,
      behind: 4,
      ahead: 4,
    },
  };
}

export function mockSyncFile(path: string): StagedFile {
  const mine = path === "0Synopsis.tex" ? SYNOPSIS.join("\n") + "\n" : chapterAt(5).join("\n") + "\n";
  const theirs = path === "0Synopsis.tex" ? SYNOPSIS.slice(0, 5).join("\n") + "\n\\end{document}\n" : THEIRS_CHAPTER;
  return { base: fileView(chapterAt(4).join("\n") + "\n"), mine: fileView(mine), theirs: fileView(theirs) };
}

export function mockSyncApply(request: ApplyRequest): Applied {
  const unresolved = mockSyncPreview(request.mode).conflicts.filter(
    (c) => !request.resolutions.some((r) => r.path === c.path),
  );
  if (unresolved.length > 0) throw new Error(`${unresolved.length} conflict(s) still need an answer`);
  ahead = [];
  versions.push(
    v(versions.length + 1, Math.random().toString(16).slice(2, 9), "Merged with Overleaf: 4 files, 1 conflict resolved", "merge", "Moss", 0, { files: 4, added: 12, removed: 3 }),
  );
  versions.forEach((x, i) => (x.current = i === versions.length - 1));
  return {
    result: "done",
    outcome: {
      pulled: 2,
      pushed: 2,
      merged: 1,
      resolved: request.resolutions.length,
      failures: [],
      aborted: null,
      left_alone: 0,
      complete: true,
      versions: [],
    },
  };
}
