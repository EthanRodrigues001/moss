// The only module that talks to the Rust side. In a plain browser (`npm run ui:dev` without
// Tauri) it falls back to an in-memory mock so the UI can be designed and screenshotted.

import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { mockMcpStatus } from "./mock-mcp";
import {
  mockHistoryFile,
  mockHistoryList,
  mockHistoryVersion,
  mockHistoryWorking,
  mockSetLinked,
  mockSyncApply,
  mockSyncFile,
  mockSyncPreview,
  mockSyncStatus,
} from "./mock-versions";
import type {
  AddReport,
  AgentSetup,
  Applied,
  ApplyRequest,
  Approval,
  ChromePairing,
  McpStatus,
  CompileSummary,
  FileEntry,
  FileView,
  ForwardHit,
  ImportReport,
  InverseHit,
  OverleafLink,
  OverleafStatus,
  ProjectPatch,
  ProjectSettings,
  ProjectSummary,
  RemoteProject,
  RemoteHistory,
  ReplaceReport,
  Rev,
  SearchOptions,
  SearchReport,
  SettingsView,
  SnapshotInfo,
  StagedFile,
  SyncMode,
  SyncPreview,
  SyncStatus,
  TexStatus,
  SelfTest,
  UpdateRelease,
  SetupPartId,
  SetupProgress,
  SetupReport,
  VersionDetail,
  WriteTestReport,
} from "./types";

export const inTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// ---------- mock (browser only) ----------

const mockProjects: ProjectSummary[] = [
  {
    dir_name: "Main_Project_Report_Template",
    id: "1",
    name: "Main Project Report",
    root_file: "0Synopsis.tex",
    origin: "overleaf",
    overleaf: { project_id: "abc123", name: "Main Project Report", last_synced: 1_760_000_000 },
    archived: false,
    last_edited_ms: Date.now() - 3_600_000,
  },
  {
    dir_name: "Notes",
    id: "2",
    name: "Reading notes",
    root_file: "main.tex",
    origin: "local",
    overleaf: null,
    archived: false,
    last_edited_ms: Date.now() - 86_400_000 * 3,
  },
];

const mockFiles: FileEntry[] = [
  { path: "chapters", is_dir: true, size: 0, kind: "folder" },
  { path: "chapters/Chapter1Introduction.tex", is_dir: false, size: 3200, kind: "tex" },
  { path: "0Synopsis.tex", is_dir: false, size: 5400, kind: "tex" },
  { path: "0Synopsis.bib", is_dir: false, size: 900, kind: "bib" },
  { path: "fcritlogo.jpg", is_dir: false, size: 41000, kind: "image" },
];

const mockText =
  "\\documentclass[12pt]{report}\n\\usepackage{graphicx}\n\\begin{document}\n\\include{chapters/Chapter1Introduction}\n\\end{document}\n";

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

let mockConnected = false;
let mockDisabled: string[] = [];
// Open the browser preview with `?approval=1` to see the Approval prompt.
const mockApprovals: Approval[] =
  typeof window !== "undefined" && window.location.search.includes("approval=1")
    ? [
        {
          id: "a1",
          project: "Main_Project_Report_Template",
          action: "delete_file",
          detail: "delete chapters/old-draft.tex",
          created_at: Math.floor(Date.now() / 1000),
          status: "pending",
        },
      ]
    : [];

const mockSettings: SettingsView = {
  running_workspace: "C:\\Users\\you\\Documents\\Moss",
  workspace: "C:\\Users\\you\\Documents\\Moss",
  workspace_is_custom: false,
  tex_bin_dir: null,
};
let mockProjectSettings: ProjectSettings = {
  root_file: "0Synopsis.tex",
  engine: "auto",
  shell_escape: false,
  trusted: false,
  root_candidates: ["0Synopsis.tex", "appendix.tex"],
};

const mockRemote: RemoteProject[] = [
  { id: "o1", name: "Main Project Report", last_updated: "2026-10-24T09:12:00Z", access_level: "owner", owner: "Ethan R", archived: false, trashed: false },
  { id: "o2", name: "Research paper draft", last_updated: "2026-10-20T17:40:00Z", access_level: "owner", owner: "Ethan R", archived: false, trashed: false },
  { id: "o3", name: "Shared lab report", last_updated: "2026-09-30T08:00:00Z", access_level: "readAndWrite", owner: "A. Student", archived: false, trashed: false },
];

// ---------- API ----------

export const api = {
  /** The running version, e.g. `0.1.0-beta.1`. */
  async appVersion(): Promise<string> {
    if (!inTauri()) return "0.1.0-dev";
    return getVersion();
  },

  /** A newer published Moss, or null when up to date. Rejects when the release channel cannot be read. */
  async updateCheck(): Promise<UpdateRelease | null> {
    if (!inTauri()) return null;
    return invoke("update_check");
  },

  /** Downloads and installs the update, then Moss restarts. Progress arrives through `onUpdateProgress`. */
  async updateInstall(): Promise<void> {
    if (!inTauri()) return;
    await invoke("update_install");
  },

  async onUpdateProgress(handler: (got: number, total: number | null) => void): Promise<() => void> {
    if (!inTauri()) return () => {};
    return listen<[number, number | null]>("update-progress", (e) => handler(e.payload[0], e.payload[1]));
  },

  async setupCheck(): Promise<SetupReport> {
    if (!inTauri())
      return {
        tex: { flavor: "MiKTeX", bin_dir: "C:/Users/you/AppData/Local/Programs/MiKTeX/miktex/bin/x64", latexmk: "latexmk.exe", perl: "perl.exe" },
        perl: "perl.exe",
        synctex: true,
        pdftoppm: true,
        auto_install: "yes",
        missing: [],
        foreign_tex: false,
      };
    return invoke("setup_check");
  },

  async setupInstall(parts: SetupPartId[], warm: boolean): Promise<TexStatus> {
    if (!inTauri()) return api.texStatus();
    return invoke("setup_install", { parts, warm });
  },

  async setupCancel(): Promise<void> {
    if (inTauri()) await invoke("setup_cancel");
  },

  async setupSelfTest(): Promise<SelfTest> {
    if (!inTauri()) return { ok: true, seconds: 1.2, pages: 1, problem: null };
    return invoke("setup_self_test");
  },

  async setupOnboarded(): Promise<boolean> {
    if (!inTauri()) return true;
    return invoke("setup_onboarded");
  },

  async setupMarkOnboarded(): Promise<void> {
    if (inTauri()) await invoke("setup_mark_onboarded");
  },

  async setupCreateTour(): Promise<ProjectSummary> {
    if (!inTauri()) return mockImport("Moss tour").then((r) => r.project);
    return invoke("setup_create_tour");
  },

  async onSetupProgress(handler: (p: SetupProgress) => void): Promise<() => void> {
    if (!inTauri()) return () => {};
    return listen<SetupProgress>("setup-progress", (e) => handler(e.payload));
  },

  async texStatus(): Promise<TexStatus> {
    if (!inTauri())
      return {
        found: true,
        install: {
          flavor: "MiKTeX",
          bin_dir: "C:/Users/you/AppData/Local/Programs/MiKTeX/miktex/bin/x64",
          latexmk: "latexmk.exe",
          perl: "C:/Strawberry/perl/bin/perl.exe",
        },
        problem: null,
      };
    return invoke("tex_status");
  },

  async workspaceRoot(): Promise<string> {
    if (!inTauri()) return "C:/Users/you/Documents/Moss";
    const info = await invoke<{ root: string }>("workspace_info");
    return info.root;
  },

  async listProjects(): Promise<ProjectSummary[]> {
    if (!inTauri()) return structuredClone(mockProjects);
    return invoke("list_projects");
  },

  async createProject(name: string): Promise<ProjectSummary> {
    if (!inTauri()) {
      const p: ProjectSummary = {
        dir_name: name,
        id: String(Date.now()),
        name,
        root_file: "main.tex",
        origin: "local",
        overleaf: null,
        archived: false,
        last_edited_ms: Date.now(),
      };
      mockProjects.push(p);
      return p;
    }
    return invoke("create_project", { name });
  },

  async setProjectArchived(project: string, archived: boolean): Promise<ProjectSummary> {
    if (!inTauri()) {
      const p = mockProjects.find((m) => m.dir_name === project);
      if (!p) throw new Error(`No project ${project}`);
      p.archived = archived;
      return structuredClone(p);
    }
    return invoke("project_set_archived", { project, archived });
  },

  /** Changes the name Moss shows; the folder keeps its name. */
  async renameProject(project: string, name: string): Promise<ProjectSummary> {
    if (!inTauri()) {
      const p = mockProjects.find((m) => m.dir_name === project);
      if (!p) throw new Error(`No project ${project}`);
      const wanted = name.trim();
      if (!wanted) throw new Error("A project needs a name.");
      if (mockProjects.some((m) => m !== p && m.name.toLowerCase() === wanted.toLowerCase()))
        throw new Error(`Another project is already called “${wanted}”.`);
      p.name = wanted;
      return structuredClone(p);
    }
    return invoke("project_rename", { project, name });
  },

  /** Deletes the project's folder from this PC. Callers confirm first. */
  async deleteProject(project: string): Promise<void> {
    if (!inTauri()) {
      const i = mockProjects.findIndex((m) => m.dir_name === project);
      if (i >= 0) mockProjects.splice(i, 1);
      return;
    }
    await invoke("project_delete", { project });
  },

  /** Opens a file picker, then imports the chosen zip. Resolves to null if cancelled. */
  async importZip(): Promise<ImportReport | null> {
    if (!inTauri()) return mockImport("Imported zip");
    const path = await open({
      multiple: false,
      filters: [{ name: "Zip archive", extensions: ["zip"] }],
    });
    if (typeof path !== "string") return null;
    return invoke("import_zip", { path });
  },

  async importFolder(): Promise<ImportReport | null> {
    if (!inTauri()) return mockImport("Imported folder");
    const path = await open({ directory: true, multiple: false });
    if (typeof path !== "string") return null;
    return invoke("import_folder", { path });
  },

  async listFiles(project: string): Promise<FileEntry[]> {
    if (!inTauri()) return structuredClone(mockFiles);
    return invoke("list_files", { project });
  },

  async readText(project: string, path: string): Promise<string> {
    if (!inTauri()) return mockText;
    return invoke("read_text_file", { project, path });
  },

  async writeText(project: string, path: string, content: string): Promise<boolean> {
    if (!inTauri()) return true;
    return invoke("write_text_file", { project, path, content });
  },

  async compile(project: string): Promise<CompileSummary> {
    if (!inTauri()) {
      await delay(900);
      return {
        success: false,
        timed_out: false,
        duration_ms: 2700,
        has_pdf: false,
        engine: "pdflatex",
        diagnostics: [
          {
            severity: "Error",
            message: "LaTeX Error: Unknown float option `H'.",
            file: "chapters/Chapter1Introduction.tex",
            line: 22,
          },
          { severity: "Warning", message: "Citation `x' undefined on input line 12.", file: null, line: 12 },
        ],
        console_tail: "Latexmk: Errors, so I did not complete making targets",
      };
    }
    return invoke("compile_project", { project });
  },

  /** The latest PDF as a blob URL; the caller must revoke it. */
  async pdfUrl(project: string): Promise<string> {
    if (!inTauri()) throw new Error("PDF preview needs the desktop app");
    const bytes = await invoke<ArrayBuffer>("read_pdf", { project });
    return URL.createObjectURL(new Blob([bytes], { type: "application/pdf" }));
  },

  /** Ask where to save the compiled PDF, then copy it there. False if the user cancelled. */
  async pdfSaveAs(project: string): Promise<boolean> {
    if (!inTauri()) throw new Error("Saving the PDF needs the desktop app");
    const dest = await save({
      defaultPath: `${project}.pdf`,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (!dest) return false;
    await invoke("pdf_save_as", { project, dest });
    return true;
  },

  /** Open the compiled PDF in the system's PDF app. */
  async pdfOpen(project: string): Promise<void> {
    if (!inTauri()) throw new Error("Opening the PDF needs the desktop app");
    await invoke("pdf_open", { project });
  },

  /** A project file as a blob URL (for image and PDF previews); the caller must revoke it. */
  async fileUrl(project: string, path: string): Promise<string> {
    if (!inTauri()) {
      if (path.toLowerCase().endsWith(".pdf")) throw new Error("PDF preview needs the desktop app");
      // A drawn placeholder so the preview layout can be checked in a plain browser.
      const canvas = document.createElement("canvas");
      canvas.width = 480;
      canvas.height = 300;
      const ctx = canvas.getContext("2d")!;
      ctx.fillStyle = "#e8eefc";
      ctx.fillRect(0, 0, 480, 300);
      ctx.fillStyle = "#346bf1";
      ctx.beginPath();
      ctx.arc(240, 150, 90, 0, Math.PI * 2);
      ctx.fill();
      const blob = await new Promise<Blob>((resolve) => canvas.toBlob((b) => resolve(b!), "image/png"));
      return URL.createObjectURL(blob);
    }
    const bytes = await invoke<ArrayBuffer>("read_binary_file", { project, path });
    const ext = path.split(".").pop()?.toLowerCase() ?? "";
    const types: Record<string, string> = {
      png: "image/png",
      jpg: "image/jpeg",
      jpeg: "image/jpeg",
      gif: "image/gif",
      svg: "image/svg+xml",
      bmp: "image/bmp",
      webp: "image/webp",
      pdf: "application/pdf",
    };
    return URL.createObjectURL(new Blob([bytes], { type: types[ext] ?? "application/octet-stream" }));
  },

  // ---------- File tree ----------

  async treeCreate(project: string, path: string, isDir: boolean): Promise<void> {
    if (!inTauri()) {
      if (mockFiles.some((f) => f.path === path)) throw new Error(`\`${path}\` already exists`);
      mockFiles.push({ path, is_dir: isDir, size: 0, kind: isDir ? "folder" : path.endsWith(".tex") ? "tex" : "asset" });
      return;
    }
    await invoke("tree_create", { project, path, isDir });
  },

  async treeRename(project: string, from: string, to: string): Promise<void> {
    if (!inTauri()) {
      for (const f of mockFiles) {
        if (f.path === from) f.path = to;
        else if (f.path.startsWith(`${from}/`)) f.path = to + f.path.slice(from.length);
      }
      return;
    }
    await invoke("tree_rename", { project, from, to });
  },

  async treeDelete(project: string, path: string): Promise<void> {
    if (!inTauri()) {
      const keep = mockFiles.filter((f) => f.path !== path && !f.path.startsWith(`${path}/`));
      mockFiles.splice(0, mockFiles.length, ...keep);
      return;
    }
    await invoke("tree_delete", { project, path });
  },

  /** Picks files on this PC and copies them into `folder`. Null when the picker is cancelled. */
  async treeAddFiles(project: string, folder: string): Promise<AddReport | null> {
    if (!inTauri()) {
      const path = `${folder ? `${folder}/` : ""}diagram.png`;
      mockFiles.push({ path, is_dir: false, size: 20000, kind: "image" });
      return { added: [path], skipped: [] };
    }
    const picked = await open({ multiple: true });
    const sources = Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
    if (sources.length === 0) return null;
    return invoke("tree_add_files", { project, folder, sources });
  },

  // ---------- Settings ----------

  async settingsGet(): Promise<SettingsView> {
    if (!inTauri()) return structuredClone(mockSettings);
    return invoke("settings_get");
  },

  /** Picks a folder, saves it as the Workspace (used after a restart). Null if cancelled. */
  async settingsChooseWorkspace(): Promise<SettingsView | null> {
    if (!inTauri()) return structuredClone(mockSettings);
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== "string") return null;
    return invoke("settings_set_workspace", { dir });
  },

  async settingsResetWorkspace(): Promise<SettingsView> {
    if (!inTauri()) return structuredClone(mockSettings);
    return invoke("settings_set_workspace", { dir: null });
  },

  /** Picks the folder with latexmk, then re-detects. Null if cancelled. */
  async settingsChooseTexDir(): Promise<TexStatus | null> {
    if (!inTauri()) return api.texStatus();
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== "string") return null;
    return invoke("settings_set_tex_dir", { dir });
  },

  async settingsResetTexDir(): Promise<TexStatus> {
    if (!inTauri()) return api.texStatus();
    return invoke("settings_set_tex_dir", { dir: null });
  },

  async projectSettingsGet(project: string): Promise<ProjectSettings> {
    if (!inTauri()) return structuredClone(mockProjectSettings);
    return invoke("project_settings_get", { project });
  },

  async projectSettingsSet(project: string, patch: ProjectPatch): Promise<ProjectSettings> {
    if (!inTauri()) {
      mockProjectSettings = { ...mockProjectSettings, ...patch };
      return structuredClone(mockProjectSettings);
    }
    return invoke("project_settings_set", { project, patch });
  },

  // ---------- SyncTeX ----------

  /** Where a source line landed in the PDF: one box per line of text. Empty when SyncTeX has no answer. */
  async synctexForward(project: string, file: string, line: number, column: number): Promise<ForwardHit[]> {
    if (!inTauri()) return [{ page: 1, x: 120, y: 300, width: 360, height: 12 }];
    return invoke("synctex_forward", { project, file, line, column });
  },

  /** The source line behind a point in the PDF. Null for text that came from the TeX install. */
  async synctexInverse(project: string, page: number, x: number, y: number): Promise<InverseHit | null> {
    if (!inTauri()) return { file: "chapters/Chapter1Introduction.tex", line: 22, column: -1 };
    return invoke("synctex_inverse", { project, page, x, y });
  },

  // ---------- Agents ----------

  /** Requests from an Agent waiting for the user's click. */
  async approvalsPending(): Promise<Approval[]> {
    if (!inTauri()) return mockApprovals.filter((a) => a.status === "pending");
    return invoke("approvals_pending");
  },

  async approvalsRespond(id: string, approve: boolean): Promise<void> {
    if (!inTauri()) {
      for (const a of mockApprovals) if (a.id === id) a.status = approve ? "approved" : "denied";
      return;
    }
    await invoke("approvals_respond", { id, approve });
  },

  /** Undo the latest change (usually an Agent's). Null when there is nothing earlier. */
  async historyUndo(project: string): Promise<SnapshotInfo | null> {
    if (!inTauri()) return null;
    return invoke("history_undo", { project });
  },

  async historyRedo(project: string): Promise<SnapshotInfo | null> {
    if (!inTauri()) return null;
    return invoke("history_redo", { project });
  },

  // ---------- Versions (the History tab) ----------

  async historyList(project: string): Promise<SnapshotInfo[]> {
    if (!inTauri()) return mockHistoryList();
    return invoke("history_list", { project });
  },

  /** Name the current files as a Version. Null when this state is already a saved Version. */
  async historySave(project: string, message: string): Promise<SnapshotInfo | null> {
    if (!inTauri()) return null;
    return invoke("history_save", { project, message });
  },

  /** What one Version changed against the one before it. */
  async historyVersion(project: string, id: number): Promise<VersionDetail> {
    if (!inTauri()) return mockHistoryVersion(id);
    return invoke("history_version", { project, id });
  },

  /** What changed since the latest Version (the Working copy row). */
  async historyWorking(project: string): Promise<VersionDetail> {
    if (!inTauri()) return mockHistoryWorking();
    return invoke("history_working", { project });
  },

  async historyFile(project: string, rev: Rev, path: string): Promise<FileView> {
    if (!inTauri()) return mockHistoryFile(rev, path);
    return invoke("history_file", { project, rev, path });
  },

  async historyRestore(project: string, id: number): Promise<SnapshotInfo> {
    if (!inTauri()) return mockHistoryList()[id - 1];
    return invoke("history_restore", { project, id });
  },

  async historyRestoreFile(project: string, id: number, path: string): Promise<SnapshotInfo> {
    if (!inTauri()) return mockHistoryList()[id - 1];
    return invoke("history_restore_file", { project, id, path });
  },

  // ---------- Sync ----------

  async syncStatus(project: string): Promise<SyncStatus> {
    if (!inTauri()) return mockSyncStatus(project);
    return invoke("sync_status", { project });
  },

  /** What a Pull, Push or Sync would do. Changes nothing. */
  async syncPreview(project: string, mode: SyncMode): Promise<SyncPreview> {
    if (!inTauri()) {
      await delay(400);
      return mockSyncPreview(mode);
    }
    return invoke("sync_preview", { project, mode });
  },

  /** One file of the open Review, on this PC, on Overleaf and in the Base snapshot. */
  async syncFile(project: string, path: string): Promise<StagedFile> {
    if (!inTauri()) return mockSyncFile(path);
    return invoke("sync_file", { project, path });
  },

  async syncApply(project: string, request: ApplyRequest): Promise<Applied> {
    if (!inTauri()) {
      await delay(700);
      return mockSyncApply(request);
    }
    return invoke("sync_apply", { project, request });
  },

  /** Overleaf's own change history (who edited what). Read-only. */
  async overleafHistory(project: string, before: number | null): Promise<RemoteHistory> {
    if (!inTauri()) {
      const now = Math.floor(Date.now() / 1000);
      return {
        updates: [
          { from_version: 40, to_version: 42, users: ["riyaraju2005"], started: now - 700, ended: now - 640, edited: ["Chapter5ResultandDiscussion.tex"], added: [], removed: [], renamed: [], labels: [] },
          { from_version: 38, to_version: 40, users: ["riyaraju2005"], started: now - 4000, ended: now - 3900, edited: [], added: ["WhatsApp Image 2026-10-25 at 9.52.35 PM.jpeg"], removed: [], renamed: [], labels: [] },
          { from_version: 30, to_version: 38, users: ["riyaraju2005", "A. Student"], started: now - 90000, ended: now - 86000, edited: ["Chapter5ResultandDiscussion.tex", "appendixA.tex"], added: [], removed: ["old.tex"], renamed: ["a.tex -> b.tex"], labels: ["Submitted draft"] },
        ],
        next_before: null,
      };
    }
    return invoke("overleaf_history", { project, before });
  },

  async syncLinkPractice(project: string): Promise<OverleafLink> {
    if (!inTauri()) {
      mockSetLinked(true);
      return { project_id: "practice:Notes", name: "Notes", last_synced: null };
    }
    return invoke("sync_link_practice", { project });
  },

  async syncLinkNew(project: string): Promise<OverleafLink> {
    if (!inTauri()) {
      mockSetLinked(true);
      return { project_id: "new123", name: project, last_synced: null };
    }
    return invoke("sync_link_new", { project });
  },

  async syncLinkExisting(project: string, overleafId: string, name: string): Promise<OverleafLink> {
    if (!inTauri()) {
      mockSetLinked(true);
      return { project_id: overleafId, name, last_synced: null };
    }
    return invoke("sync_link_existing", { project, overleafId, name });
  },

  async syncUnlink(project: string): Promise<void> {
    if (!inTauri()) {
      mockSetLinked(false);
      return;
    }
    await invoke("sync_unlink", { project });
  },

  /** Make a Practice project (sample files in a folder that plays Overleaf) and Pull it. */
  async practiceCreate(name: string): Promise<ImportReport> {
    if (!inTauri()) return mockImport(name);
    return invoke("practice_create", { name });
  },

  async agentSetup(): Promise<AgentSetup> {
    if (!inTauri()) {
      const command = "C:\\Program Files\\Moss\\moss-mcp.exe";
      const workspace = "C:\\Users\\you\\Documents\\Moss";
      return {
        command,
        workspace,
        config: JSON.stringify(
          { mcpServers: { moss: { command, args: ["--workspace", workspace] } } },
          null,
          2,
        ),
        claude_code: `claude mcp add moss -- "${command}" --workspace "${workspace}"`,
      };
    }
    return invoke("agent_setup");
  },

  /** The MCP server's status, its tools, which AI programs are set up and what agents did lately. */
  async mcpStatus(): Promise<McpStatus> {
    if (!inTauri()) {
      await delay(300);
      return mockMcpStatus(mockDisabled);
    }
    return invoke("mcp_status");
  },

  /** Switch tools on or off for agents (from their next session). Returns every switched-off tool. */
  async mcpSetToolsEnabled(names: string[], enabled: boolean): Promise<string[]> {
    if (!inTauri()) {
      const off = new Set(mockDisabled);
      for (const n of names) {
        if (enabled) off.delete(n);
        else off.add(n);
      }
      mockDisabled = [...off].sort();
      return mockDisabled;
    }
    return invoke("mcp_set_tools_enabled", { names, enabled });
  },

  // ---------- Overleaf ----------

  async overleafStatus(): Promise<OverleafStatus> {
    if (!inTauri())
      return { connected: mockConnected, email: mockConnected ? "you@example.com" : null, host: "www.overleaf.com" };
    return invoke("overleaf_status");
  },

  async overleafOpenLogin(): Promise<void> {
    if (!inTauri()) {
      await delay(1200);
      mockConnected = true;
      window.dispatchEvent(new CustomEvent("moss-mock-overleaf-connected"));
      return;
    }
    await invoke("overleaf_open_login");
  },

  async overleafConnectPasted(text: string): Promise<OverleafStatus> {
    if (!inTauri()) {
      await delay(500);
      mockConnected = true;
      return { connected: true, email: "you@example.com", host: "www.overleaf.com" };
    }
    return invoke("overleaf_connect_pasted", { text });
  },

  async overleafSignOut(): Promise<void> {
    if (!inTauri()) {
      mockConnected = false;
      return;
    }
    await invoke("overleaf_sign_out");
  },

  async overleafProjects(): Promise<RemoteProject[]> {
    if (!inTauri()) {
      await delay(400);
      return structuredClone(mockRemote);
    }
    return invoke("overleaf_projects");
  },

  async overleafPull(projectId: string, name: string): Promise<ImportReport> {
    if (!inTauri()) {
      await delay(900);
      const project: ProjectSummary = {
        dir_name: name,
        id: projectId,
        name,
        root_file: "main.tex",
        origin: "overleaf",
        overleaf: { project_id: projectId, name, last_synced: Math.floor(Date.now() / 1000) },
        archived: false,
        last_edited_ms: Date.now(),
      };
      mockProjects.push(project);
      return { project, files_imported: 16, ignored: 0, skipped: [], warnings: [] };
    }
    return invoke("overleaf_pull", { projectId, name });
  },

  async overleafWriteTest(projectId: string): Promise<WriteTestReport> {
    if (!inTauri()) {
      await delay(1200);
      return {
        ok: true,
        steps: [
          { name: "Read project file tree", ok: true, detail: "17 entries, root folder found", ms: 640 },
          { name: "Upload test file", ok: true, detail: "created doc 66a1", ms: 480 },
          { name: "Confirm it appears", ok: true, detail: "found in the project tree", ms: 610 },
          { name: "Delete test file", ok: true, detail: "deleted", ms: 350 },
          { name: "Confirm it is gone", ok: true, detail: "project is back to how it was", ms: 590 },
        ],
      };
    }
    return invoke("overleaf_write_test", { projectId });
  },

  /** Opens the short-lived local listener the Chrome extension posts to; returns the code to type. */
  async overleafStartChromePairing(): Promise<ChromePairing> {
    if (!inTauri()) {
      await delay(300);
      window.setTimeout(() => window.dispatchEvent(new CustomEvent("moss-mock-overleaf-connected")), 4000);
      mockConnected = true;
      return { code: "482913", port: 47831, seconds: 180, extension_dir: "C:/Moss/extensions/chrome" };
    }
    return invoke("overleaf_start_chrome_pairing");
  },

  async overleafStopChromePairing(): Promise<void> {
    if (!inTauri()) return;
    await invoke("overleaf_stop_chrome_pairing");
  },

  /** Search every text file of the Project. */
  async searchProject(project: string, options: SearchOptions): Promise<SearchReport> {
    if (!inTauri()) return { files: [], total: 0, files_searched: 0, truncated: false };
    return invoke("search_project", { project, options });
  },

  /** Replace in every matching file, or only in `paths`. One restore point is taken first. */
  async replaceInProject(
    project: string,
    options: SearchOptions,
    replacement: string,
    paths?: string[],
  ): Promise<ReplaceReport> {
    if (!inTauri()) return { files: [], replacements: 0, restore_point: false };
    return invoke("replace_in_project", { project, options, replacement, paths: paths ?? null });
  },

  /** Open a web link or mail address in the default app (a PDF link, never inside the webview). */
  async openExternal(url: string): Promise<void> {
    if (!inTauri()) {
      window.open(url, "_blank", "noopener");
      return;
    }
    await invoke("open_external", { url });
  },

  async revealInExplorer(path: string): Promise<void> {
    if (!inTauri()) return;
    await invoke("reveal_in_explorer", { path });
  },

  /** Calls back when the sign-in window finishes (connected) or is closed without signing in. */
  async onOverleafLogin(handlers: {
    connected: (s: OverleafStatus) => void;
    closed: () => void;
  }): Promise<() => void> {
    if (!inTauri()) {
      const h = () =>
        handlers.connected({ connected: true, email: "you@example.com", host: "www.overleaf.com" });
      window.addEventListener("moss-mock-overleaf-connected", h);
      return () => window.removeEventListener("moss-mock-overleaf-connected", h);
    }
    const offA = await listen<OverleafStatus>("overleaf-connected", (e) => handlers.connected(e.payload));
    const offB = await listen("overleaf-login-closed", () => handlers.closed());
    return () => {
      offA();
      offB();
    };
  },
};

async function mockImport(name: string): Promise<ImportReport> {
  await delay(500);
  const project: ProjectSummary = {
    dir_name: name,
    id: String(Date.now()),
    name,
    root_file: "main.tex",
    origin: "local",
    overleaf: null,
    archived: false,
    last_edited_ms: Date.now(),
  };
  mockProjects.push(project);
  return {
    project,
    files_imported: 18,
    ignored: 2,
    skipped: [{ path: "chapters/aux.tex", reason: "`aux.tex` is a reserved Windows device name" }],
    warnings: [],
  };
}
