"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { History as HistoryIcon, Loader2, PanelLeftOpen, Play, Redo2, Search, Undo2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Tip } from "./tip";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { api } from "@/lib/moss-api";
import type {
  CompileSummary,
  FileEntry,
  ImportReport,
  ProjectSummary,
  RemoteProject,
  SyncMode,
  SyncStatus,
  TexStatus,
} from "@/lib/types";
import { cn } from "@/lib/utils";
import { useStoredFlag } from "@/lib/use-stored-flag";
import { AddProjectDialog } from "./add-project-dialog";
import { DialogsProvider, useDialogs } from "./dialogs";
import { EditorTabs } from "./editor-tabs";
import { FileOutline } from "./file-outline";
import { ApprovalPrompt } from "./approval-prompt";
import { ConnectOverleafDialog } from "./connect-overleaf-dialog";
import { HistoryView } from "./history-view";
import { SyncMenu } from "./sync-menu";
import { SyncReviewDialog } from "./sync-review-dialog";
import { EditorPane, type EditorHandle } from "./editor-pane";
import { FileTree, isEditable } from "./file-tree";
import { ImportReportDialog } from "./import-report-dialog";
import { OverleafDialog } from "./overleaf-dialog";
import { PreviewPane } from "./preview-pane";
import type { PdfHighlight } from "./pdf-viewer";
import { ProjectSidebar } from "./project-sidebar";
import { syncAnchor } from "@/lib/source-anchor";
import { FilePreview } from "./file-preview";
import { SearchDialog } from "./search-dialog";
import { SetupDialog } from "./setup-dialog";
import { useUpdater } from "@/lib/use-updater";
import { Welcome } from "./welcome";
import { SettingsDialog, type SettingsPage } from "./settings-dialog";

const AUTOSAVE_MS = 700;
const EXTERNAL_CHANGE_POLL_MS = 2000;
const FOLLOW_DELAY_MS = 350;
const SYNC_POLL_MS = 15_000;

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The whole app, inside the provider that gives it Moss's own prompt and confirm dialogs. */
export function MossApp() {
  return (
    <DialogsProvider>
      <MossShell />
    </DialogsProvider>
  );
}

function MossShell() {
  const dialogs = useDialogs();
  const [addOpen, setAddOpen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchSeed, setSearchSeed] = useState("");
  const [sidebarCollapsed, setSidebarCollapsed] = useStoredFlag("moss.sidebarCollapsed", false);
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [tex, setTex] = useState<TexStatus | null>(null);
  const [busy, setBusy] = useState(false);

  const [files, setFiles] = useState<FileEntry[]>([]);
  const [active, setActive] = useState<FileEntry | null>(null);
  // Files opened this session, as tabs. Kept per Project; a file that no longer exists drops out.
  const [tabState, setTabState] = useState<{ dir: string; paths: string[] }>({ dir: "", paths: [] });
  const [text, setText] = useState("");
  const [dirty, setDirty] = useState(false);

  const [compiling, setCompiling] = useState(false);
  const [result, setResult] = useState<CompileSummary | null>(null);
  const [pdfUrl, setPdfUrl] = useState<string | null>(null);
  const [report, setReport] = useState<ImportReport | null>(null);
  const [overleafOpen, setOverleafOpen] = useState(false);
  const [setupOpen, setSetupOpen] = useState(false);
  const updater = useUpdater();
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsPage, setSettingsPage] = useState<SettingsPage>("general");
  const [view, setView] = useState<"editor" | "history">("editor");
  const [syncState, setSyncState] = useState<{ dir: string; status: SyncStatus } | null>(null);
  const [reviewMode, setReviewMode] = useState<SyncMode | null>(null);
  const [reviewNonce, setReviewNonce] = useState(0);
  const [connectOpen, setConnectOpen] = useState(false);
  const [historyKey, setHistoryKey] = useState(0);
  const [highlight, setHighlight] = useState<PdfHighlight | null>(null);
  const jumpToken = useRef(0);
  const [follow, setFollow] = useState(true);
  const followTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastFollowSpot = useRef("");

  const editor = useRef<EditorHandle>(null);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Latest values for callbacks that must not go stale (autosave, key handlers).
  const live = useRef({ selected, active, text, dirty });
  useEffect(() => {
    live.current = { selected, active, text, dirty };
  });

  const project = projects.find((p) => p.dir_name === selected) ?? null;
  const syncStatus = syncState && syncState.dir === selected ? syncState.status : null;

  const openSidebarButton = (
    <Tip label="Show projects" side="bottom">
      <Button variant="ghost" size="icon-xs" aria-label="Show projects" onClick={() => setSidebarCollapsed(false)}>
        <PanelLeftOpen />
      </Button>
    </Tip>
  );

  const flushSave = useCallback(async () => {
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = null;
    const { selected, active, text, dirty } = live.current;
    if (!dirty || !selected || !active) return;
    try {
      await api.writeText(selected, active.path, text);
      setDirty(false);
    } catch (e) {
      toast.error(`Could not save ${active.path}: ${message(e)}`);
    }
  }, []);

  const refreshProjects = useCallback(async () => {
    const list = await api.listProjects();
    setProjects(list);
    return list;
  }, []);

  useEffect(() => {
    api.listProjects().then(setProjects).catch((e) => toast.error(message(e)));
    api.texStatus().then(setTex).catch((e) => toast.error(message(e)));
    // First run: open setup once, whether or not LaTeX is already here (it also offers the tour).
    api.setupOnboarded().then((done) => setSetupOpen(!done)).catch(() => {});
  }, []);

  useEffect(() => {
    return () => {
      if (pdfUrl) URL.revokeObjectURL(pdfUrl);
    };
  }, [pdfUrl]);

  /** Which files differ from the last Sync, and whether the Project is linked at all. */
  const refreshSync = useCallback(async () => {
    const dir = live.current.selected;
    if (!dir) return;
    try {
      const status = await api.syncStatus(dir);
      if (live.current.selected === dir) setSyncState({ dir, status });
    } catch {
      // The Project may have just been removed; the next poll tries again.
    }
  }, []);

  useEffect(() => {
    if (!selected) return;
    void refreshSync();
    const timer = setInterval(() => {
      if (!document.hidden) void refreshSync();
    }, SYNC_POLL_MS);
    return () => clearInterval(timer);
  }, [selected, refreshSync]);

  const openFile = useCallback(
    async (file: FileEntry, dirName?: string) => {
      const dir = dirName ?? live.current.selected;
      if (!dir) return;
      await flushSave();
      setActive(file);
      setTabState((prev) =>
        prev.dir !== dir
          ? { dir, paths: [file.path] }
          : prev.paths.includes(file.path)
            ? prev
            : { dir, paths: [...prev.paths, file.path] },
      );
      setDirty(false);
      if (!isEditable(file.kind)) {
        setText("");
        return;
      }
      try {
        setText(await api.readText(dir, file.path));
      } catch (e) {
        setText("");
        toast.error(`Could not open ${file.path}: ${message(e)}`);
      }
    },
    [flushSave],
  );

  // Files opened this session, as tabs. Kept per Project; a file that no longer exists drops out.
  const openTabs =
    tabState.dir === selected
      ? tabState.paths.flatMap((path) => files.find((f) => !f.is_dir && f.path === path) ?? [])
      : [];

  async function closeTab(file: FileEntry) {
    const paths = openTabs.map((t) => t.path);
    const at = paths.indexOf(file.path);
    setTabState((prev) => ({ ...prev, paths: prev.paths.filter((p) => p !== file.path) }));
    if (active?.path !== file.path) return;
    const next = openTabs[at + 1] ?? openTabs[at - 1];
    if (next) {
      await openFile(next);
      return;
    }
    await flushSave();
    setActive(null);
    setText("");
    setDirty(false);
  }

  const selectProject = useCallback(
    async (dirName: string, summary?: ProjectSummary) => {
      await flushSave();
      setSelected(dirName);
      setView("editor");
      setResult(null);
      setPdfUrl(null);
      setActive(null);
      setText("");
      try {
        const list = await api.listFiles(dirName);
        setFiles(list);
        const rootFile = (summary ?? projects.find((p) => p.dir_name === dirName))?.root_file;
        const root = list.find((f) => f.path === rootFile);
        if (root) await openFile(root, dirName);
        // Show the PDF from the last compile, if there is one, without compiling again.
        api
          .pdfUrl(dirName)
          .then((url) => {
            if (live.current.selected === dirName) setPdfUrl(url);
            else URL.revokeObjectURL(url);
          })
          .catch(() => {});
      } catch (e) {
        toast.error(message(e));
      }
    },
    [flushSave, openFile, projects],
  );

  async function runImport(pick: () => Promise<ImportReport | null>) {
    setBusy(true);
    try {
      const imported = await pick();
      if (!imported) return;
      await refreshProjects();
      await selectProject(imported.project.dir_name, imported.project);
      if (imported.skipped.length > 0 || imported.warnings.length > 0) setReport(imported);
      else toast.success(`Imported “${imported.project.name}” (${imported.files_imported} files)`);
    } catch (e) {
      toast.error(`Import failed: ${message(e)}`);
    } finally {
      setBusy(false);
    }
  }

  /** Pull an Overleaf project into a new Overleaf-tagged Project. True when it worked. */
  async function pullFromOverleaf(remote: RemoteProject): Promise<boolean> {
    setBusy(true);
    try {
      const imported = await api.overleafPull(remote.id, remote.name);
      await refreshProjects();
      await selectProject(imported.project.dir_name, imported.project);
      if (imported.skipped.length > 0 || imported.warnings.length > 0) setReport(imported);
      else toast.success(`Imported “${imported.project.name}” from Overleaf (${imported.files_imported} files)`);
      return true;
    } catch (e) {
      toast.error(`Could not import from Overleaf: ${message(e)}`);
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function createProject(name: string) {
    setBusy(true);
    try {
      const created = await api.createProject(name);
      await refreshProjects();
      await selectProject(created.dir_name, created);
    } catch (e) {
      toast.error(message(e));
    } finally {
      setBusy(false);
    }
  }

  /** Ask for a name, then create a file or folder inside `folder`. */
  async function newEntry(folder: string, isDir: boolean) {
    const dir = live.current.selected;
    if (!dir) return;
    const name = await dialogs.prompt({
      title: isDir ? "New folder" : "New file",
      description: folder ? `It will be created in ${folder}/.` : "It will be created in the project root.",
      label: isDir ? "Folder name" : "File name",
      initial: isDir ? "figures" : "untitled.tex",
      confirmLabel: "Create",
    });
    const clean = name?.trim().replace(/^\/+/, "");
    if (!clean) return;
    const path = `${folder ? `${folder}/` : ""}${clean}`;
    try {
      await flushSave();
      await api.treeCreate(dir, path, isDir);
      const list = await api.listFiles(dir);
      setFiles(list);
      const created = list.find((f) => f.path === path);
      if (created && !isDir) await openFile(created, dir);
    } catch (e) {
      toast.error(message(e));
    }
  }

  async function addFilesFromPc(folder: string) {
    const dir = live.current.selected;
    if (!dir) return;
    try {
      const added = await api.treeAddFiles(dir, folder);
      if (!added) return;
      setFiles(await api.listFiles(dir));
      if (added.added.length > 0) toast.success(`Added ${added.added.length} file${added.added.length === 1 ? "" : "s"}`);
      for (const s of added.skipped) toast.warning(`${s.name} was skipped: ${s.reason}`);
    } catch (e) {
      toast.error(message(e));
    }
  }

  async function renameEntry(file: FileEntry) {
    const dir = live.current.selected;
    if (!dir) return;
    const parent = file.path.split("/").slice(0, -1).join("/");
    const oldName = file.path.split("/").pop() ?? file.path;
    const name = await dialogs.prompt({
      title: `Rename ${file.is_dir ? "folder" : "file"}`,
      label: "New name",
      initial: oldName,
      confirmLabel: "Rename",
    });
    const clean = name?.trim();
    if (!clean || clean === oldName) return;
    const to = `${parent ? `${parent}/` : ""}${clean}`;
    try {
      await flushSave();
      await api.treeRename(dir, file.path, to);
      const list = await api.listFiles(dir);
      setFiles(list);
      const open = live.current.active;
      if (open) {
        const moved =
          open.path === file.path
            ? to
            : open.path.startsWith(`${file.path}/`)
              ? to + open.path.slice(file.path.length)
              : open.path;
        if (moved !== open.path) setActive(list.find((f) => f.path === moved) ?? null);
      }
      void refreshProjects(); // the Root file may have moved
    } catch (e) {
      toast.error(message(e));
    }
  }

  async function deleteEntry(file: FileEntry) {
    const dir = live.current.selected;
    if (!dir) return;
    const name = file.path.split("/").pop() ?? file.path;
    const isRoot = project?.root_file === file.path;
    const yes = await dialogs.confirm({
      title: `Delete ${name}?`,
      description: `${file.is_dir ? "The folder and everything in it will be removed. " : ""}${
        isRoot ? "This is the main file that gets compiled. " : ""
      }You can bring it back with Undo.`,
      confirmLabel: "Delete",
      destructive: true,
    });
    if (!yes) return;
    try {
      const open = live.current.active;
      const openIsGone = open && (open.path === file.path || open.path.startsWith(`${file.path}/`));
      if (openIsGone) {
        if (saveTimer.current) clearTimeout(saveTimer.current);
        saveTimer.current = null;
        setDirty(false);
        setActive(null);
        setText("");
      } else {
        await flushSave();
      }
      await api.treeDelete(dir, file.path);
      setFiles(await api.listFiles(dir));
    } catch (e) {
      toast.error(message(e));
    }
  }

  function onEdit(value: string) {
    setText(value);
    setDirty(true);
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => void flushSave(), AUTOSAVE_MS);
  }

  /** Files already given a restore point this session before their first visual-editor edit. */
  const visualCheckpoints = useRef(new Set<string>());

  /** The visual editor rewrites LaTeX, so keep a Version of the file as it was before the first edit. */
  function onVisualEdit(value: string) {
    const { selected, active } = live.current;
    if (selected && active) {
      const key = `${selected}/${active.path}`;
      if (!visualCheckpoints.current.has(key)) {
        visualCheckpoints.current.add(key);
        void flushSave()
          .then(() => api.historySave(selected, `Before visual editing of ${active.path}`))
          .catch(() => visualCheckpoints.current.delete(key));
      }
    }
    onEdit(value);
  }

  const compile = useCallback(async () => {
    const dir = live.current.selected;
    if (!dir) return;
    await flushSave();
    setCompiling(true);
    try {
      const summary = await api.compile(dir);
      setResult(summary);
      if (summary.has_pdf) {
        try {
          setPdfUrl(await api.pdfUrl(dir));
        } catch (e) {
          toast.error(message(e));
        }
      }
      if (summary.success) toast.success(`Compiled in ${(summary.duration_ms / 1000).toFixed(1)} s`);
    } catch (e) {
      toast.error(message(e));
    } finally {
      setCompiling(false);
    }
  }, [flushSave]);

  /** Pick up changes made outside the editor (an Agent, undo, Sync) unless there are unsaved edits. */
  const syncFromDisk = useCallback(async () => {
    const dir = live.current.selected;
    if (!dir) return;
    try {
      const list = await api.listFiles(dir);
      if (live.current.selected !== dir) return;
      const key = (l: FileEntry[]) => l.map((f) => `${f.path}:${f.size}`).join("|");
      setFiles((prev) => (key(prev) === key(list) ? prev : list));

      const { active, dirty } = live.current;
      if (!active || dirty || !isEditable(active.kind)) return;
      if (!list.some((f) => f.path === active.path)) {
        setActive(null);
        setText("");
        return;
      }
      const disk = await api.readText(dir, active.path);
      if (live.current.selected !== dir || live.current.dirty) return;
      setText((current) => (current === disk ? current : disk));
    } catch {
      // The project may have been deleted or is mid-write; the next tick tries again.
    }
  }, []);

  /** Files, the log or the link changed (a Sync, a restore, a link): reload what depends on them. */
  const reloadAfterChange = useCallback(async () => {
    await syncFromDisk();
    await refreshSync();
    setHistoryKey((k) => k + 1);
  }, [syncFromDisk, refreshSync]);

  async function unlinkProject(target: ProjectSummary | null = project) {
    if (!target) return;
    const practice = target.dir_name === selected && syncStatus?.practice;
    const ok = await dialogs.confirm({
      title: `Unlink “${target.name}” from ${practice ? "its practice project" : "Overleaf"}?`,
      description: "Your files stay exactly as they are. The project becomes Local, and Moss forgets what was last synced.",
      confirmLabel: "Unlink",
      destructive: true,
    });
    if (!ok) return;
    try {
      await api.syncUnlink(target.dir_name);
      await refreshProjects();
      if (target.dir_name === selected) await reloadAfterChange();
      toast.success("Unlinked");
    } catch (e) {
      toast.error(message(e));
    }
  }

  async function renameProject(target: ProjectSummary) {
    const name = await dialogs.prompt({
      title: "Rename project",
      description: target.origin === "overleaf"
        ? "Only the name in Moss changes. The folder and the Overleaf project keep their names."
        : "Only the name in Moss changes. The folder on disk keeps its name.",
      label: "Name",
      initial: target.name,
      confirmLabel: "Rename",
    });
    if (name === null || name.trim() === target.name) return;
    try {
      await api.renameProject(target.dir_name, name);
      await refreshProjects();
    } catch (e) {
      toast.error(message(e));
    }
  }

  async function archiveProject(target: ProjectSummary, archived: boolean) {
    try {
      await api.setProjectArchived(target.dir_name, archived);
      await refreshProjects();
      toast.success(archived ? `Archived “${target.name}”` : `Moved “${target.name}” back to Projects`);
    } catch (e) {
      toast.error(message(e));
    }
  }

  async function deleteProject(target: ProjectSummary) {
    const linked = target.origin === "overleaf";
    const ok = await dialogs.confirm({
      title: `Delete “${target.name}”?`,
      description: linked
        ? "This removes the project folder from this PC, including its version history. It cannot be undone. The copy on Overleaf is not touched."
        : "This removes the project folder from this PC, including its version history. It cannot be undone. Archive it instead if you might want it back.",
      confirmLabel: "Delete project",
      destructive: true,
      requireText: target.name,
    });
    if (!ok) return;
    try {
      if (target.dir_name === selected) {
        if (saveTimer.current) clearTimeout(saveTimer.current);
        saveTimer.current = null;
        setDirty(false);
        live.current = { ...live.current, dirty: false };
        setSelected(null);
        setFiles([]);
        setActive(null);
        setText("");
        setResult(null);
        setPdfUrl(null);
      }
      await api.deleteProject(target.dir_name);
      await refreshProjects();
      toast.success(`Deleted “${target.name}”`);
    } catch (e) {
      toast.error(`Could not delete: ${message(e)}`);
      await refreshProjects();
    }
  }

  async function createPractice() {
    setBusy(true);
    try {
      const made = await api.practiceCreate("Practice project");
      await refreshProjects();
      await selectProject(made.project.dir_name, made.project);
      toast.success("Practice project ready. Edit a file in its practice folder, then Pull.");
    } catch (e) {
      toast.error(message(e));
    } finally {
      setBusy(false);
    }
  }

  const startSync = (mode: SyncMode) => {
    setReviewNonce((n) => n + 1);
    setReviewMode(mode);
  };

  const syncMenu = (
    <SyncMenu
      status={syncStatus}
      onSync={startSync}
      onConnect={() => setConnectOpen(true)}
      onUnlink={() => void unlinkProject()}
      onOpenPracticeFolder={() => {
        if (syncStatus?.practice_folder) void api.revealInExplorer(syncStatus.practice_folder);
      }}
    />
  );

  useEffect(() => {
    if (!selected) return;
    const timer = setInterval(() => void syncFromDisk(), EXTERNAL_CHANGE_POLL_MS);
    return () => clearInterval(timer);
  }, [selected, syncFromDisk]);

  /** SyncTeX forward search: show a source line in the PDF. */
  async function showInPdf(line: number, column: number, quiet = false) {
    const { selected, active } = live.current;
    if (!selected || !active) return;
    if (!pdfUrl) {
      if (quiet) return;
      toast.message("Compile first, then the PDF can be searched");
      return;
    }
    try {
      const anchor = syncAnchor(live.current.text.split("\n"), line, column);
      const hits = await api.synctexForward(selected, active.path, anchor.line, anchor.column);
      if (hits.length === 0) {
        if (!quiet) {
          toast.message("No PDF position for this line (it may be a comment or not in the output)");
        }
        return;
      }
      jumpToken.current += 1;
      setHighlight({ hits, fraction: anchor.fraction, token: jumpToken.current, quiet });
    } catch (e) {
      if (!quiet) toast.error(message(e));
    }
  }

  /** Keep the PDF at the caret: once it settles on a new line, scroll there quietly. */
  function onCursorMove(line: number, column: number) {
    const spot = `${live.current.active?.path}:${line}`;
    if (!follow || !pdfUrl || spot === lastFollowSpot.current) return;
    lastFollowSpot.current = spot;
    if (followTimer.current) clearTimeout(followTimer.current);
    followTimer.current = setTimeout(() => void showInPdf(line, column, true), FOLLOW_DELAY_MS);
  }

  async function stepHistory(kind: "undo" | "redo") {
    const dir = live.current.selected;
    if (!dir) return;
    await flushSave();
    try {
      const step = kind === "undo" ? await api.historyUndo(dir) : await api.historyRedo(dir);
      if (!step) {
        toast.message(`Nothing to ${kind}`);
        return;
      }
      toast.success(`Restored “${step.label}”`);
      await syncFromDisk();
    } catch (e) {
      toast.error(message(e));
    }
  }

  const jumpTo = useCallback(
    async (path: string, line: number | null) => {
      const file = files.find((f) => f.path === path);
      if (!file) {
        toast.error(`${path} is not in this project`);
        return;
      }
      await openFile(file);
      if (line) setTimeout(() => editor.current?.goToLine(line), 60);
    },
    [files, openFile],
  );

  /** SyncTeX inverse search: a double-click in the PDF opens the source line. */
  async function openFromPdf(page: number, x: number, y: number) {
    const dir = live.current.selected;
    if (!dir) return;
    try {
      const hit = await api.synctexInverse(dir, page, x, y);
      if (!hit) {
        toast.message("That text does not come from your project files");
        return;
      }
      await jumpTo(hit.file, hit.line);
    } catch (e) {
      toast.error(message(e));
    }
  }

  /** Open "Search in project", starting with the editor's one-line selection. Files are saved first. */
  const openSearch = useCallback(async () => {
    if (!live.current.selected) return;
    setSearchSeed(editor.current?.getSelection() ?? "");
    await flushSave();
    setSearchOpen(true);
  }, [flushSave]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void flushSave();
      } else if (mod && e.shiftKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        void openSearch();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [flushSave, openSearch]);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <ProjectSidebar
        projects={projects}
        selected={selected}
        tex={tex}
        updater={updater}
        busy={busy}
        collapsed={sidebarCollapsed}
        onCollapsedChange={setSidebarCollapsed}
        onSelect={(d) => void selectProject(d)}
        onAddProject={() => setAddOpen(true)}
        onRename={(p) => void renameProject(p)}
        onArchive={(p, archived) => void archiveProject(p, archived)}
        onDelete={(p) => void deleteProject(p)}
        onUnlink={(p) => void unlinkProject(p)}
        onOpenAgents={() => {
          setSettingsPage("mcp");
          setSettingsOpen(true);
        }}
        onOpenSettings={() => {
          setSettingsPage("general");
          setSettingsOpen(true);
        }}
      />

      <main className="flex min-w-0 flex-1 flex-col">
        {project ? (
          <>
            <header className="flex h-12 shrink-0 items-center gap-2 border-b px-4">
              {sidebarCollapsed && openSidebarButton}
              <div className="flex min-w-0 flex-1 items-center gap-2">
                <span className="grid size-5 shrink-0 place-items-center rounded bg-primary/15 text-[10px] font-bold text-primary">
                  MS
                </span>
                <span className="shrink-0 text-sm text-muted-foreground">Moss</span>
                <span className="shrink-0 text-muted-foreground/60">/</span>
                <Tip label={project.name} side="bottom">
                  <h2 className="min-w-0 truncate text-sm font-semibold">{project.name}</h2>
                </Tip>
                {dirty && <span className="shrink-0 text-xs text-muted-foreground">Unsaved…</span>}
              </div>

              <div className="flex shrink-0 items-center gap-1">
                <Tip label={view === "history" ? "Back to the editor" : "History"} side="bottom">
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label="History"
                    aria-pressed={view === "history"}
                    data-testid="tab-history"
                    className={cn(view === "history" && "bg-accent text-foreground")}
                    onClick={() => {
                      if (view === "history") setView("editor");
                      else void flushSave().then(() => setView("history"));
                    }}
                  >
                    <HistoryIcon />
                  </Button>
                </Tip>
                <Tip label="Search in project (Ctrl+Shift+F)" side="bottom">
                  <Button
                    variant="outline"
                    size="sm"
                    aria-label="Search in this project"
                    className="w-8 justify-start gap-2 px-2 text-muted-foreground lg:w-48"
                    onClick={() => void openSearch()}
                  >
                    <Search />
                    <span className="hidden flex-1 text-left lg:inline">Search in this project</span>
                  </Button>
                </Tip>
              </div>

              <Separator orientation="vertical" className="mx-1 h-5" />

              <div className="flex shrink-0 items-center gap-1">
                <Tip label="Undo the last change (including an agent's)" side="bottom">
                  <Button variant="ghost" size="icon-sm" aria-label="Undo" onClick={() => void stepHistory("undo")}>
                    <Undo2 />
                  </Button>
                </Tip>
                <Tip label="Redo" side="bottom">
                  <Button variant="ghost" size="icon-sm" aria-label="Redo" onClick={() => void stepHistory("redo")}>
                    <Redo2 />
                  </Button>
                </Tip>
              </div>

              <Separator orientation="vertical" className="mx-1 h-5" />

              <div className="flex shrink-0 items-center gap-2">
                {syncMenu}
                <Button
                  size="sm"
                  onClick={() => (tex !== null && !tex.found ? setSetupOpen(true) : void compile())}
                  disabled={compiling}
                >
                  {compiling ? (
                    <Loader2 data-icon="inline-start" className="animate-spin" />
                  ) : (
                    <Play data-icon="inline-start" />
                  )}
                  {tex !== null && !tex.found ? "Set up LaTeX" : "Compile"}
                </Button>
              </div>
            </header>

            {view === "history" ? (
              <div className="min-h-0 flex-1">
                <HistoryView
                  project={project}
                  refreshKey={historyKey}
                  onFilesChanged={() => void reloadAfterChange()}
                  hasOverleafHistory={Boolean(syncStatus?.link) && !syncStatus?.practice}
                />
              </div>
            ) : (
            <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1">
              <ResizablePanel defaultSize="18%" minSize="12%">
                <div className="flex h-full min-h-0 flex-col">
                  <FileTree
                    files={files}
                    rootFile={project.root_file}
                    active={active?.path ?? null}
                    onOpen={(f) => void openFile(f)}
                    onNewFile={(folder) => void newEntry(folder, false)}
                    onNewFolder={(folder) => void newEntry(folder, true)}
                    onUpload={(folder) => void addFilesFromPc(folder)}
                    onRename={(f) => void renameEntry(f)}
                    onDelete={(f) => void deleteEntry(f)}
                  />
                  <FileOutline
                    source={active?.kind === "tex" ? text : null}
                    onJump={(line) => editor.current?.goToLine(line)}
                  />
                </div>
              </ResizablePanel>
              <ResizableHandle />
              <ResizablePanel defaultSize="40%" minSize="20%">
                <div className="flex h-full min-h-0 flex-col">
                <EditorTabs
                  tabs={openTabs}
                  active={active?.path ?? null}
                  onSelect={(f) => void openFile(f)}
                  onClose={(f) => void closeTab(f)}
                />
                <div className="min-h-0 flex-1">
                {active && !isEditable(active.kind) ? (
                  <FilePreview key={`${project.dir_name}/${active.path}`} project={project.dir_name} file={active} />
                ) : (
                  <EditorPane
                    ref={editor}
                    path={active?.path ?? null}
                    kind={active?.kind ?? null}
                    editable={active ? isEditable(active.kind) : false}
                    value={text}
                    problems={result?.diagnostics}
                    onChange={onEdit}
                    onVisualChange={onVisualEdit}
                    onCompile={() => void compile()}
                    onShowInPdf={(l, c) => void showInPdf(l, c)}
                    onCursor={onCursorMove}
                  />
                )}
                </div>
                </div>
              </ResizablePanel>
              <ResizableHandle withHandle />
              <ResizablePanel defaultSize="46%" minSize="20%">
                <PreviewPane
                  pdfUrl={pdfUrl}
                  result={result}
                  compiling={compiling}
                  onJump={(f, l) => void jumpTo(f, l)}
                  highlight={highlight}
                  onInverse={(p, x, y) => void openFromPdf(p, x, y)}
                  follow={follow}
                  onFollowChange={setFollow}
                  project={selected}
                />
              </ResizablePanel>
            </ResizablePanelGroup>
            )}
          </>
        ) : (
          <>
            {sidebarCollapsed && (
              <header className="flex h-12 shrink-0 items-center border-b px-4">{openSidebarButton}</header>
            )}
          <Welcome tex={tex} busy={busy} onAdd={() => setAddOpen(true)} onSetup={() => setSetupOpen(true)} />
          </>
        )}
      </main>

      <AddProjectDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        onCreate={(name) => void createProject(name)}
        onImportZip={() => void runImport(api.importZip)}
        onImportFolder={() => void runImport(api.importFolder)}
        onOverleaf={() => setOverleafOpen(true)}
        onPractice={() => void createPractice()}
      />
      <SearchDialog
        open={searchOpen}
        onOpenChange={setSearchOpen}
        project={selected}
        initialQuery={searchSeed}
        beforeReplace={flushSave}
        onOpenHit={(path, line) => {
          setView("editor");
          void jumpTo(path, line);
        }}
        onReplaced={() => void reloadAfterChange()}
      />
      <SetupDialog
        open={setupOpen}
        onOpenChange={setSetupOpen}
        onTexChange={setTex}
        onTour={(tour) => {
          void refreshProjects().then(() => selectProject(tour.dir_name, tour));
        }}
        onOverleaf={() => {
          setSetupOpen(false);
          setOverleafOpen(true);
        }}
        onAgents={() => {
          setSetupOpen(false);
          setSettingsPage("mcp");
          setSettingsOpen(true);
        }}
      />
      <ApprovalPrompt onAnswered={() => void syncFromDisk()} />
      <SettingsDialog
        open={settingsOpen}
        onOpenChange={setSettingsOpen}
        page={settingsPage}
        onPageChange={setSettingsPage}
        project={project}
        tex={tex}
        onTexChange={setTex}
        updater={updater}
        onSetup={() => {
          setSettingsOpen(false);
          setSetupOpen(true);
        }}
        onProjectChanged={() => void refreshProjects()}
      />
      <ImportReportDialog report={report} onClose={() => setReport(null)} />
      {project && reviewMode && (
        <SyncReviewDialog
          key={`${project.dir_name}:${reviewMode}:${reviewNonce}`}
          mode={reviewMode}
          project={project.dir_name}
          projectName={project.name}
          onClose={() => setReviewMode(null)}
          onApplied={() => void reloadAfterChange()}
        />
      )}
      {project && (
        <ConnectOverleafDialog
          open={connectOpen}
          onOpenChange={setConnectOpen}
          project={project.dir_name}
          projectName={project.name}
          onSignIn={() => setOverleafOpen(true)}
          onLinked={() => {
            void refreshProjects();
            void reloadAfterChange();
          }}
        />
      )}
      <OverleafDialog
        open={overleafOpen}
        onOpenChange={setOverleafOpen}
        localProjects={projects}
        onPull={pullFromOverleaf}
      />
    </div>
  );
}
