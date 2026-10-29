"use client";

import { Bot, Cloud, FileText, Leaf } from "lucide-react";
import { relativeTime } from "@/lib/time";
import type { SnapshotInfo, VersionKind } from "@/lib/types";
import { cn } from "@/lib/utils";

/** The row that stands for the files on disk right now. */
export const WORKING = "working" as const;
export type Selection = number | typeof WORKING;

type Props = {
  /** Newest first. */
  versions: SnapshotInfo[];
  /** Files changed since the latest Version, or 0 when the Working copy is clean. */
  workingFiles: number;
  workingLines: { added: number; removed: number };
  selected: Selection | null;
  onSelect: (selection: Selection) => void;
};

const KIND_LABEL: Partial<Record<VersionKind, { text: string; className: string }>> = {
  manual: { text: "Saved", className: "text-primary" },
  agent: { text: "Agent", className: "text-violet-400" },
  pull: { text: "Pull", className: "text-emerald-400" },
  push: { text: "Push", className: "text-sky-400" },
  merge: { text: "Merge", className: "text-amber-400" },
  restore: { text: "Restore", className: "text-orange-400" },
  external: { text: "Outside Moss", className: "text-rose-400" },
};

const DOT: Record<VersionKind, string> = {
  auto: "bg-primary",
  manual: "bg-primary",
  agent: "bg-violet-400",
  pull: "bg-emerald-400",
  push: "bg-sky-400",
  merge: "bg-amber-400",
  restore: "bg-orange-400",
  external: "bg-rose-400",
};

function Avatar({ author }: { author: string }) {
  const base = "flex size-[18px] shrink-0 items-center justify-center rounded-[5px] text-[9px] font-bold";
  if (author === "Agent")
    return (
      <span className={cn(base, "bg-violet-500/20 text-violet-300")}>
        <Bot className="size-3" />
      </span>
    );
  if (author === "Overleaf")
    return (
      <span className={cn(base, "bg-emerald-500/20 text-emerald-300")}>
        <Leaf className="size-3" />
      </span>
    );
  if (author === "Moss")
    return (
      <span className={cn(base, "bg-amber-500/20 text-amber-300")}>
        <Cloud className="size-3" />
      </span>
    );
  return <span className={cn(base, "bg-primary/25 text-primary")}>{author.slice(0, 1).toUpperCase()}</span>;
}

export function Changes({ files, added, removed }: { files: number; added: number; removed: number }) {
  return (
    <span className="inline-flex items-center justify-end gap-2 font-mono text-xs tabular-nums">
      <span className="inline-flex items-center gap-1 text-muted-foreground">
        <FileText className="size-3.5" />
        {files}
      </span>
      {(added > 0 || removed > 0) && <span className="text-muted-foreground/60">•</span>}
      {added > 0 && <span className="font-semibold text-emerald-400">+{added}</span>}
      {removed > 0 && <span className="font-semibold text-destructive">-{removed}</span>}
    </span>
  );
}

const COLS = "grid-cols-[28px_76px_minmax(0,1fr)_150px_96px_150px]";

export function VersionLog({ versions, workingFiles, workingLines, selected, onSelect }: Props) {
  const rows: Array<{ key: Selection; working: boolean; v?: SnapshotInfo }> = [];
  if (workingFiles > 0) rows.push({ key: WORKING, working: true });
  versions.forEach((v) => rows.push({ key: v.id, working: false, v }));

  return (
    <div role="table" aria-label="Versions" className="min-w-[640px] text-sm">
      <div
        role="row"
        className={cn(
          "sticky top-0 z-10 grid h-9 items-center gap-x-3 border-b bg-background px-3 text-[11px] font-semibold tracking-[0.14em] text-muted-foreground uppercase",
          COLS,
        )}
      >
        <span />
        <span role="columnheader">SHA</span>
        <span role="columnheader">Subject</span>
        <span role="columnheader">Author</span>
        <span role="columnheader">Date</span>
        <span role="columnheader" className="text-right">
          Changes
        </span>
      </div>

      {rows.length === 0 && (
        <p className="px-4 py-8 text-center text-muted-foreground">
          No versions yet. Changes you make are saved here automatically, and “Save version” names them.
        </p>
      )}

      {rows.map((row, i) => {
        const isSelected = selected === row.key;
        const first = i === 0;
        const last = i === rows.length - 1;
        return (
          <button
            key={String(row.key)}
            type="button"
            role="row"
            data-testid={row.working ? "row-working" : "row-version"}
            data-selected={isSelected}
            onClick={() => onSelect(row.key)}
            className={cn(
              "relative grid h-10 w-full items-center gap-x-3 px-3 text-left transition-colors hover:bg-accent/60",
              isSelected && "bg-accent",
              COLS,
            )}
          >
            <span className="relative flex h-full items-center justify-center" aria-hidden>
              {!(first && last) && (
                <span
                  className={cn(
                    "absolute left-1/2 w-px -translate-x-1/2 bg-primary/70",
                    first ? "top-1/2 bottom-0" : last ? "top-0 bottom-1/2" : "top-0 bottom-0",
                  )}
                />
              )}
              <span
                className={cn(
                  "relative size-2.5 rounded-full ring-2 ring-background",
                  row.working ? "border border-dashed border-primary bg-background" : DOT[row.v!.kind],
                )}
              />
            </span>

            {row.working ? (
              <>
                <span className="font-mono text-xs text-muted-foreground">——</span>
                <span className="truncate font-medium">
                  Working copy
                  <span className="ml-2 text-xs font-normal text-muted-foreground">not saved as a version yet</span>
                </span>
                <span className="flex items-center gap-2 text-muted-foreground">
                  <Avatar author="You" />
                  You
                </span>
                <span className="text-xs text-muted-foreground">now</span>
                <Changes files={workingFiles} added={workingLines.added} removed={workingLines.removed} />
              </>
            ) : (
              <>
                <span className="font-mono text-xs text-muted-foreground">{row.v!.sha}</span>
                <span className="flex min-w-0 items-center gap-2">
                  <span className="truncate font-medium" title={row.v!.label}>
                    {row.v!.label}
                  </span>
                  {KIND_LABEL[row.v!.kind] && (
                    <span
                      className={cn(
                        "shrink-0 rounded-md border px-1.5 text-[10px] leading-4 font-semibold uppercase",
                        KIND_LABEL[row.v!.kind]!.className,
                      )}
                    >
                      {KIND_LABEL[row.v!.kind]!.text}
                    </span>
                  )}
                  {row.v!.current && (
                    <span className="shrink-0 rounded-md bg-primary/15 px-1.5 text-[10px] leading-4 font-semibold text-primary uppercase">
                      Current
                    </span>
                  )}
                </span>
                <span className="flex min-w-0 items-center gap-2">
                  <Avatar author={row.v!.author} />
                  <span className="truncate">{row.v!.author}</span>
                </span>
                <span className="font-mono text-xs text-muted-foreground" title={new Date(row.v!.created_at * 1000).toLocaleString()}>
                  {relativeTime(row.v!.created_at)}
                </span>
                <Changes files={row.v!.stats.files} added={row.v!.stats.added} removed={row.v!.stats.removed} />
              </>
            )}
          </button>
        );
      })}
    </div>
  );
}
