"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { Bookmark, History as HistoryIcon, RotateCcw, Undo2 } from "lucide-react";
import { OverleafHistory } from "./overleaf-history";
import { cn } from "@/lib/utils";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { ScrollArea } from "@/components/ui/scroll-area";
import { api } from "@/lib/moss-api";
import { fullDate, plural } from "@/lib/time";
import type { ProjectSummary, SnapshotInfo, VersionDetail } from "@/lib/types";
import { useDialogs } from "./dialogs";
import { type DiffPair, FileDiff } from "./file-diff";
import { Changes, type Selection, VersionLog, WORKING } from "./version-log";

type Props = {
  project: ProjectSummary;
  /** Bump to reload after something outside this tab changed the files or the log. */
  refreshKey: number;
  /** Files on disk changed (restore, discard): the editor and file tree should reload. */
  onFilesChanged: () => void;
  /** Where the Sync buttons go (they also live in the project header). */
  /** The Project is linked to a real Overleaf project, so its own history can be shown. */
  hasOverleafHistory: boolean;
};

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

type Loaded = { versions: SnapshotInfo[]; working: VersionDetail };

export function HistoryView({ project, refreshKey, onFilesChanged, hasOverleafHistory }: Props) {
  const [source, setSource] = useState<"moss" | "overleaf">("moss");
  const showOverleaf = hasOverleafHistory && source === "overleaf";
  const dialogs = useDialogs();
  const dir = project.dir_name;
  const [tick, setTick] = useState(0);
  const [data, setData] = useState<Loaded | null>(null);
  const [selected, setSelected] = useState<Selection | null>(null);
  const [detail, setDetail] = useState<{ key: Selection; value: VersionDetail } | null>(null);

  useEffect(() => {
    let cancelled = false;
    Promise.all([api.historyList(dir), api.historyWorking(dir)]).then(
      ([versions, working]) => !cancelled && setData({ versions, working }),
      (e) => !cancelled && toast.error(`Could not load the history: ${message(e)}`),
    );
    return () => {
      cancelled = true;
    };
  }, [dir, refreshKey, tick]);

  const newestFirst = useMemo(() => (data ? [...data.versions].reverse() : []), [data]);
  const workingCount = data?.working.files.length ?? 0;
  const workingLines = useMemo(
    () => ({
      added: data?.working.files.reduce((n, f) => n + f.added, 0) ?? 0,
      removed: data?.working.files.reduce((n, f) => n + f.removed, 0) ?? 0,
    }),
    [data],
  );

  // What is shown: the user's pick if it still exists, else the Working copy or the newest Version.
  const effective: Selection | null = useMemo(() => {
    if (!data) return null;
    if (selected === WORKING && workingCount > 0) return WORKING;
    if (typeof selected === "number" && data.versions.some((v) => v.id === selected)) return selected;
    if (workingCount > 0) return WORKING;
    return newestFirst[0]?.id ?? null;
  }, [data, selected, workingCount, newestFirst]);

  useEffect(() => {
    if (effective === null || effective === WORKING || !data) return;
    let cancelled = false;
    api.historyVersion(dir, effective).then(
      (value) => !cancelled && setDetail({ key: effective, value }),
      (e) => !cancelled && toast.error(`Could not load this version: ${message(e)}`),
    );
    return () => {
      cancelled = true;
    };
  }, [dir, effective, data]);

  const shown: VersionDetail | null =
    effective === WORKING ? (data?.working ?? null) : detail && detail.key === effective ? detail.value : null;
  const version = typeof effective === "number" ? data?.versions.find((v) => v.id === effective) : undefined;
  const current = data?.versions.find((v) => v.current);

  const reload = () => setTick((t) => t + 1);

  async function saveVersion() {
    const text = await dialogs.prompt({
      title: "Save version",
      description: "Describe what this version is, like a commit message.",
      label: "Message",
      placeholder: "Finished chapter 2",
      confirmLabel: "Save",
    });
    if (!text?.trim()) return;
    try {
      const saved = await api.historySave(dir, text.trim());
      if (saved) toast.success(`Saved version ${saved.sha}`);
      else toast.message("This state is already saved as a version.");
      reload();
    } catch (e) {
      toast.error(message(e));
    }
  }

  async function restoreVersion(v: SnapshotInfo) {
    const ok = await dialogs.confirm({
      title: `Restore version ${v.sha}?`,
      description: `Your files go back to “${v.label}”. What you have now is saved first, so you can undo this.`,
      confirmLabel: "Restore",
    });
    if (!ok) return;
    try {
      await api.historyRestore(dir, v.id);
      toast.success(`Restored version ${v.sha}`);
      reload();
      onFilesChanged();
    } catch (e) {
      toast.error(message(e));
    }
  }

  async function restoreFile(v: SnapshotInfo, path: string) {
    const ok = await dialogs.confirm({
      title: `Restore ${path}?`,
      description: `This file goes back to how it was in version ${v.sha}. Other files stay as they are, and the current state is saved first.`,
      confirmLabel: "Restore file",
    });
    if (!ok) return;
    try {
      await api.historyRestoreFile(dir, v.id, path);
      toast.success(`Restored ${path}`);
      reload();
      onFilesChanged();
    } catch (e) {
      toast.error(message(e));
    }
  }

  async function discardWorking() {
    if (!current) return;
    const ok = await dialogs.confirm({
      title: "Discard your unsaved changes?",
      description: `Files go back to the latest version (${current.sha}). The changes are kept in the history, so Undo brings them back.`,
      confirmLabel: "Discard changes",
      destructive: true,
    });
    if (!ok) return;
    try {
      await api.historyRestore(dir, current.id);
      reload();
      onFilesChanged();
    } catch (e) {
      toast.error(message(e));
    }
  }

  const loadPair = useCallback(
    (d: VersionDetail, path: string): (() => Promise<DiffPair>) =>
      async () => {
        const [original, modified] = await Promise.all([
          api.historyFile(dir, d.from, path),
          api.historyFile(dir, d.to, path),
        ]);
        return { original, modified };
      },
    [dir],
  );

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="history-view">
      <div className="flex h-11 shrink-0 items-center gap-3 border-b px-4">
        <HistoryIcon className="size-4 text-muted-foreground" />
        <h3 className="text-sm font-semibold">History</h3>
        {hasOverleafHistory && (
          <div className="flex items-center rounded-lg bg-muted p-0.5 text-xs font-medium" role="tablist" aria-label="History source">
            {(["moss", "overleaf"] as const).map((id) => (
              <button
                key={id}
                type="button"
                role="tab"
                aria-selected={source === id}
                data-testid={`history-source-${id}`}
                onClick={() => setSource(id)}
                className={cn(
                  "rounded-md px-2.5 py-1 transition-colors",
                  source === id ? "bg-background text-foreground shadow-sm" : "text-muted-foreground hover:text-foreground",
                )}
              >
                {id === "moss" ? "This PC" : "On Overleaf"}
              </button>
            ))}
          </div>
        )}
        {!showOverleaf && (
          <span className="text-xs text-muted-foreground">
            {data ? plural(data.versions.length, "version") : "Loading…"}
          </span>
        )}
        <div className="ml-auto flex items-center gap-2">
          {!showOverleaf && (
            <Button variant="outline" size="sm" onClick={() => void saveVersion()}>
              <Bookmark data-icon="inline-start" />
              Save version
            </Button>
          )}
        </div>
      </div>

      {showOverleaf ? (
        <div className="min-h-0 flex-1">
          <OverleafHistory project={dir} />
        </div>
      ) : (
      <ResizablePanelGroup orientation="vertical" className="min-h-0 flex-1">
        <ResizablePanel defaultSize="40%" minSize="15%">
          <ScrollArea className="h-full">
            {data && (
              <VersionLog
                versions={newestFirst}
                workingFiles={workingCount}
                workingLines={workingLines}
                selected={effective}
                onSelect={setSelected}
              />
            )}
          </ScrollArea>
        </ResizablePanel>
        <ResizableHandle withHandle />
        <ResizablePanel defaultSize="60%" minSize="20%">
          <ScrollArea className="h-full">
            {shown && effective !== null ? (
              <div data-testid="version-detail">
                <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b px-4 py-3">
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-semibold">
                      {effective === WORKING ? "Working copy" : version?.label}
                    </p>
                    <p className="mt-0.5 text-xs text-muted-foreground">
                      {effective === WORKING
                        ? "Changes since the latest version. Nothing here is lost: it is kept when you Undo or Restore."
                        : `${version?.sha} · ${version?.author} · ${version ? fullDate(version.created_at) : ""}`}
                    </p>
                  </div>
                  <Changes
                    files={shown.files.length}
                    added={shown.files.reduce((n, f) => n + f.added, 0)}
                    removed={shown.files.reduce((n, f) => n + f.removed, 0)}
                  />
                  {effective === WORKING ? (
                    <Button variant="outline" size="sm" onClick={() => void discardWorking()} disabled={!current}>
                      <Undo2 data-icon="inline-start" />
                      Discard changes
                    </Button>
                  ) : (
                    version && (
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={version.current}
                        title={version.current ? "This is where your files are now" : undefined}
                        onClick={() => void restoreVersion(version)}
                      >
                        <RotateCcw data-icon="inline-start" />
                        Restore this version
                      </Button>
                    )
                  )}
                </div>

                {shown.files.length === 0 ? (
                  <p className="px-4 py-8 text-center text-sm text-muted-foreground">
                    {effective === WORKING
                      ? "No changes since the latest version."
                      : "This version records an event and changed no files."}
                  </p>
                ) : (
                  shown.files.map((f, i) => (
                    <FileDiff
                      key={`${String(effective)}:${f.path}`}
                      path={f.path}
                      kind={f.kind}
                      added={f.added}
                      removed={f.removed}
                      binary={f.binary}
                      defaultOpen={i < 3}
                      load={loadPair(shown, f.path)}
                      actions={
                        version && f.kind !== "deleted" ? (
                          <Button
                            variant="ghost"
                            size="xs"
                            onClick={() => void restoreFile(version, f.path)}
                            title="Put just this file back to how it is in this version"
                          >
                            <RotateCcw data-icon="inline-start" />
                            Restore file
                          </Button>
                        ) : undefined
                      }
                    />
                  ))
                )}
              </div>
            ) : (
              <p className="px-4 py-8 text-center text-sm text-muted-foreground">
                {data ? "Select a version to see what it changed." : "Loading…"}
              </p>
            )}
          </ScrollArea>
        </ResizablePanel>
      </ResizablePanelGroup>
      )}
    </div>
  );
}
