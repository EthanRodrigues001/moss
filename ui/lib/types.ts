// Mirrors the serde output of the Rust commands in src-tauri/src/lib.rs and crates/moss-core.

export type Origin = "local" | "overleaf";

export type OverleafLink = {
  project_id: string;
  name: string;
  last_synced: number | null;
};

export type ProjectSummary = {
  dir_name: string;
  id: string;
  name: string;
  root_file: string;
  origin: Origin;
  overleaf: OverleafLink | null;
  archived: boolean;
  last_edited_ms: number | null;
};

export type FileKind =
  | "tex"
  | "bib"
  | "style"
  | "class"
  | "bst"
  | "image"
  | "pdf"
  | "asset"
  | "folder";

export type FileEntry = {
  path: string;
  is_dir: boolean;
  size: number;
  kind: FileKind;
};

export type Skipped = { path: string; reason: string };

export type ImportReport = {
  project: ProjectSummary;
  files_imported: number;
  ignored: number;
  skipped: Skipped[];
  warnings: string[];
};

export type Diagnostic = {
  severity: "Error" | "Warning";
  message: string;
  file: string | null;
  line: number | null;
};

export type CompileSummary = {
  success: boolean;
  timed_out: boolean;
  duration_ms: number;
  has_pdf: boolean;
  engine: string;
  diagnostics: Diagnostic[];
  console_tail: string;
};

export type TexInstall = {
  flavor: "MiKTeX" | "TeXLive" | "Unknown";
  bin_dir: string;
  latexmk: string;
  perl: string | null;
};

export type TexStatus = {
  found: boolean;
  install: TexInstall | null;
  problem: string | null;
};

export type UpdateRelease = { version: string; current: string; notes: string; pre_release: boolean };

export type SetupPartId = "miktex" | "perl";

export type SetupPart = { id: SetupPartId; name: string; size: number };

export type SetupReport = {
  tex: TexInstall | null;
  perl: string | null;
  synctex: boolean;
  pdftoppm: boolean;
  auto_install: "yes" | "ask" | "no" | null;
  missing: SetupPart[];
  foreign_tex: boolean;
};

export type SetupProgress = {
  part: SetupPartId;
  phase: "download" | "verify" | "install" | "configure";
  done: number;
  total: number | null;
  note: string;
};

export type SelfTest = { ok: boolean; seconds: number; pages: number | null; problem: string | null };

export type OverleafStatus = {
  connected: boolean;
  email: string | null;
  host: string;
};

export type RemoteProject = {
  id: string;
  name: string;
  last_updated: string | null;
  access_level: string | null;
  owner: string | null;
  archived: boolean;
  trashed: boolean;
};

export type TestStep = { name: string; ok: boolean; detail: string; ms: number };

export type WriteTestReport = { ok: boolean; steps: TestStep[] };

export type ChromePairing = {
  code: string;
  port: number;
  seconds: number;
  extension_dir: string | null;
};

export type Approval = {
  id: string;
  project: string;
  action: string;
  detail: string;
  created_at: number;
  status: "pending" | "approved" | "denied" | "expired";
};

export type VersionKind =
  | "auto"
  | "manual"
  | "agent"
  | "pull"
  | "push"
  | "merge"
  | "restore"
  | "external";

export type VersionStats = { files: number; added: number; removed: number };

/** One Version in the History tab (a History snapshot). */
export type SnapshotInfo = {
  id: number;
  /** Seven hex characters. */
  sha: string;
  /** The subject line. */
  label: string;
  kind: VersionKind;
  author: string;
  created_at: number;
  file_count: number;
  stats: VersionStats;
  current: boolean;
};

export type Rev = { type: "empty" } | { type: "snapshot"; id: number } | { type: "working" };

export type ChangeKind = "added" | "modified" | "deleted";

export type FileChange = {
  path: string;
  kind: ChangeKind;
  binary: boolean;
  added: number;
  removed: number;
};

/** A file at one revision, for the diff viewer. */
export type FileView = {
  exists: boolean;
  binary: boolean;
  size: number;
  text: string | null;
  too_large: boolean;
};

export type VersionDetail = { from: Rev; to: Rev; files: FileChange[] };

// ----- Sync -----

export type LocalChange = { path: string; kind: ChangeKind };

export type SyncStatus = {
  link: OverleafLink | null;
  practice: boolean;
  practice_folder: string | null;
  changes: LocalChange[];
};

export type SyncMode = "pull" | "push" | "sync";

export type Region =
  | { type: "stable"; text: string }
  | { type: "conflict"; base: string; mine: string; theirs: string };

export type Choice =
  | { type: "mine" }
  | { type: "theirs" }
  | { type: "both" }
  | { type: "text"; text: string };

export type FileChoice =
  | { type: "mine" }
  | { type: "theirs" }
  | { type: "regions"; choices: Choice[] }
  | { type: "text"; text: string };

export type ConflictKind = "both_modified" | "both_added" | "modified_deleted" | "deleted_modified";

/** Mine is this PC, Theirs is Overleaf. */
export type Conflict = {
  path: string;
  kind: ConflictKind;
  binary: boolean;
  mine_exists: boolean;
  theirs_exists: boolean;
  base: string | null;
  mine: string | null;
  theirs: string | null;
  regions: Region[] | null;
};

export type Resolution = { path: string; choice: FileChoice };

export type SyncItem = {
  path: string;
  from: string | null;
  direction: "pull" | "push" | "both";
  op: "add" | "modify" | "delete" | "rename" | "merge" | "conflict";
  applies: boolean;
  note: string | null;
  binary: boolean;
  lines: { added: number; removed: number };
};

export type SyncSummary = {
  pull: number;
  push: number;
  merge: number;
  conflict: number;
  delete_here: number;
  delete_there: number;
  behind: number;
  ahead: number;
};

export type SyncPreview = {
  token: string;
  mode: SyncMode;
  items: SyncItem[];
  conflicts: Conflict[];
  needs_attention: Skipped[];
  warnings: string[];
  mass_delete: { side: "here" | "overleaf"; count: number; total: number } | null;
  blocked: string | null;
  summary: SyncSummary;
};

export type ApplyRequest = {
  mode: SyncMode;
  token: string;
  resolutions: Resolution[];
  allow_mass_delete: boolean;
  allow_markers: boolean;
  skip: string[];
  actor?: string;
};

export type SyncOutcome = {
  pulled: number;
  pushed: number;
  merged: number;
  resolved: number;
  failures: { path: string; error: string }[];
  aborted: string | null;
  left_alone: number;
  complete: boolean;
  versions: SnapshotInfo[];
};

export type Applied =
  | { result: "stale"; side: "overleaf" | "here" }
  | { result: "done"; outcome: SyncOutcome };

/** One entry of Overleaf's own change history. */
export type RemoteUpdate = {
  from_version: number;
  to_version: number;
  users: string[];
  started: number;
  ended: number;
  edited: string[];
  added: string[];
  removed: string[];
  renamed: string[];
  labels: string[];
};

export type RemoteHistory = { updates: RemoteUpdate[]; next_before: number | null };

export type StagedFile = { base: FileView; mine: FileView; theirs: FileView };

export type AgentSetup = {
  command: string | null;
  workspace: string;
  config: string;
  claude_code: string;
};

/** One MCP tool as `moss-mcp --list-tools` describes it. */
export type McpTool = {
  name: string;
  title: string;
  description: string;
  group: string;
  /** Changes nothing. */
  read_only: boolean;
  /** Can delete or overwrite. */
  destructive: boolean;
  /** Talks to Overleaf. */
  online: boolean;
  /** May wait for the user's click. */
  needs_approval: boolean;
};

/** An AI program that can use Moss, and whether it is set up to. */
export type McpClient = {
  name: string;
  config_file: string;
  installed: boolean;
  configured: boolean;
  command: string | null;
  /** Its `moss` entry runs the `moss-mcp.exe` next to this app. */
  current: boolean;
};

export type McpActivity = { at: number; project: string; action: string; detail: string };

export type McpNeed = { name: string; ok: boolean; detail: string };

export type McpStatus = {
  state: "ready" | "missing" | "broken";
  exe: string | null;
  version: string | null;
  problem: string | null;
  workspace: string;
  tools: McpTool[];
  /** Names of tools switched off in Settings. */
  disabled: string[];
  needs: McpNeed[];
  clients: McpClient[];
  activity: McpActivity[];
  pending_approvals: number;
};

/** A spot in the PDF, in points from the page's top-left corner. */
export type ForwardHit = {
  page: number;
  x: number;
  y: number;
  width: number;
  height: number;
};

export type InverseHit = {
  file: string;
  line: number;
  column: number;
};

export type SettingsView = {
  /** The folder Moss is using right now. */
  running_workspace: string;
  /** The folder it will use after a restart. */
  workspace: string;
  workspace_is_custom: boolean;
  tex_bin_dir: string | null;
};

export type Engine = "auto" | "pdflatex" | "xelatex" | "lualatex";

export type ProjectSettings = {
  root_file: string;
  engine: Engine;
  shell_escape: boolean;
  trusted: boolean;
  root_candidates: string[];
};

export type ProjectPatch = Partial<
  Pick<ProjectSettings, "root_file" | "engine" | "shell_escape" | "trusted">
>;

export type AddReport = {
  /** Project-relative paths of the files that were added. */
  added: string[];
  skipped: { name: string; reason: string }[];
};

/** What to look for in "Search in project" (see `moss_core::search`). */
export type SearchOptions = {
  query: string;
  case_sensitive: boolean;
  whole_word: boolean;
  regex: boolean;
  /** Globs separated by commas or spaces; a leading `!` excludes. Empty means every text file. */
  include: string;
};

/** One matching line; `ranges` are character offsets of the matches inside `text`. */
export type SearchLine = { line: number; text: string; ranges: [number, number][] };

export type SearchFile = {
  path: string;
  count: number;
  lines: SearchLine[];
  lines_truncated: boolean;
};

export type SearchReport = {
  files: SearchFile[];
  total: number;
  files_searched: number;
  truncated: boolean;
};

export type ReplaceReport = {
  files: { path: string; replacements: number }[];
  replacements: number;
  restore_point: boolean;
};
