// Browser-only stand-in for the MCP status the Rust side reports (see `moss-mcp --list-tools`),
// so the Settings page can be designed without the desktop app.

import type { McpStatus, McpTool } from "./types";

// name, title, group, flags: r read-only, d destructive, o online, a needs approval
const TOOLS: [string, string, string, string, string][] = [
  ["list_projects", "List projects", "Projects", "r", "List every Project with its folder name, Root file and Origin."],
  ["create_project", "Create a project", "Projects", "", "Create a new Local Project, empty or from a template."],
  ["list_templates", "List templates", "Projects", "r", "The starter documents create_project can use."],
  ["import_project", "Import a project", "Projects", "", "Import a .zip file or a folder from this PC as a new Local Project."],
  ["project_info", "Project details", "Projects", "r", "Path, Origin, Root file, engine, shell-escape, trusted, history size."],
  ["list_files", "List files", "Reading", "r", "Every file and folder in a Project with its size."],
  ["read_file", "Read files", "Reading", "r", "Read one file whole or by line range, or several files in one call. Long files are read part by part."],
  ["search", "Search text", "Reading", "r", "Find text across a Project's text files, with path and line."],
  ["outline", "Document outline", "Reading", "r", "Sections across all included files, with file and line."],
  ["references", "Check references", "Reading", "r", "Broken \\ref and \\cite, unused labels and bibliography entries."],
  ["write_file", "Write a file", "Writing", "", "Create a file or replace its whole content. Identical content changes nothing."],
  ["write_files", "Write several files", "Writing", "", "Create or replace up to 50 files as one change."],
  ["edit_file", "Edit a file", "Writing", "", "Replace exact text without resending the file; several edits at once, all or nothing."],
  ["move_file", "Move or rename", "Writing", "", "Rename or move a file or folder."],
  ["delete_file", "Delete files", "Writing", "da", "Delete files or folders, one Approval for the lot. Undo brings them back."],
  ["compile", "Compile", "Build", "", "Compile the Root file with your TeX install."],
  ["diagnostics", "Errors and warnings", "Build", "r", "The last compile's errors, warnings and badly set boxes."],
  ["set_compile_options", "Set compile options", "Build", "", "Change the Root file or the engine."],
  ["set_shell_escape", "Allow shell-escape", "Build", "a", "Let documents run programs while compiling. Needs approval to turn on."],
  ["clean_build", "Clean build files", "Build", "", "Delete generated build files. Sources are not touched."],
  ["view_pages", "View pages", "Preview", "r", "Compiled pages as images: one page, a range, a list or all, in one image or separate."],
  ["pdf_info", "PDF info", "Preview", "r", "Page count and sizes, and whether the PDF is out of date."],
  ["view_image", "View an image", "Preview", "r", "Look at a figure file from the Project."],
  ["source_to_pdf", "Source line to PDF page", "Preview", "r", "Which page a source line landed on (SyncTeX)."],
  ["pdf_to_source", "PDF point to source line", "Preview", "r", "Which source line produced a point on a page (SyncTeX)."],
  ["history_list", "List restore points", "History", "r", "The Project's version log on this PC."],
  ["history_show", "Show a version", "History", "r", "The files one version changed."],
  ["history_changes", "Changes since a version", "History", "r", "Files added, modified or deleted since a restore point."],
  ["history_save", "Save a version", "History", "", "Name the current files as a version."],
  ["history_restore_file", "Restore one file", "History", "d", "Put one file back to how it was in a version."],
  ["restore", "Restore the project", "History", "d", "Put the whole Project back to a restore point."],
  ["undo", "Undo", "History", "d", "Step back to the state before the latest change."],
  ["redo", "Redo", "History", "d", "Step forward again after an undo."],
  ["overleaf_status", "Overleaf status", "Overleaf", "ro", "Whether the Project is linked and what changed since the last sync."],
  ["overleaf_projects", "List Overleaf projects", "Overleaf", "ro", "Your Overleaf projects, to pick one to link."],
  ["overleaf_history", "Overleaf history", "Overleaf", "ro", "Who edited what on Overleaf, newest first."],
  ["overleaf_link", "Link to Overleaf", "Overleaf", "oa", "Link a Local Project to an Overleaf project, or create one."],
  ["overleaf_unlink", "Unlink from Overleaf", "Overleaf", "oa", "Make the Project Local again. Files are untouched."],
  ["overleaf_review", "Review a sync", "Overleaf", "ro", "What a pull, push or sync would do. Changes nothing."],
  ["overleaf_conflicts", "Check for conflicts", "Overleaf", "ro", "Merge conflicts before syncing, region by region."],
  ["overleaf_pull", "Pull from Overleaf", "Overleaf", "o", "Bring Overleaf's changes here, merging line by line."],
  ["overleaf_push", "Push to Overleaf", "Overleaf", "oa", "Send this PC's changes to Overleaf."],
  ["overleaf_resolve", "Resolve conflicts", "Overleaf", "oa", "Answer the conflicts a sync listed, then finish it."],
  ["overleaf_sync", "Sync with Overleaf", "Overleaf", "oa", "Pull and push in one go."],
];

export const mockMcpTools: McpTool[] = TOOLS.map(([name, title, group, flags, description]) => ({
  name,
  title,
  group,
  description,
  read_only: flags.includes("r"),
  destructive: flags.includes("d"),
  online: flags.includes("o"),
  needs_approval: flags.includes("a"),
}));

export function mockMcpStatus(disabled: string[]): McpStatus {
  const now = Math.floor(Date.now() / 1000);
  return {
    state: "ready",
    exe: "C:\\Program Files\\Moss\\moss-mcp.exe",
    version: "0.1.0",
    problem: null,
    workspace: "C:\\Users\\you\\Documents\\Moss",
    tools: mockMcpTools,
    disabled,
    needs: [
      { name: "TeX install", ok: true, detail: "C:\\Users\\you\\AppData\\Local\\Programs\\MiKTeX\\miktex\\bin\\x64" },
      { name: "Page images (Poppler)", ok: true, detail: "found next to the TeX install" },
    ],
    clients: [
      { name: "Claude Code", config_file: "C:\\Users\\you\\.claude.json", installed: true, configured: true, command: "C:\\Program Files\\Moss\\moss-mcp.exe", current: true },
      { name: "Claude Desktop", config_file: "C:\\Users\\you\\AppData\\Roaming\\Claude\\claude_desktop_config.json", installed: true, configured: true, command: "C:\\old\\moss-mcp.exe", current: false },
      { name: "Cursor", config_file: "C:\\Users\\you\\.cursor\\mcp.json", installed: false, configured: false, command: null, current: false },
      { name: "Codex", config_file: "C:\\Users\\you\\.codex\\config.toml", installed: true, configured: false, command: null, current: false },
    ],
    activity: [
      { at: now - 90, project: "Reading notes", action: "edit_file", detail: "main.tex" },
      { at: now - 400, project: "Reading notes", action: "write_files", detail: "chapters/one.tex, chapters/two.tex, refs.bib" },
      { at: now - 7200, project: "Main Project Report", action: "delete_file", detail: "chapters/old-draft.tex" },
    ],
    pending_approvals: 0,
  };
}
