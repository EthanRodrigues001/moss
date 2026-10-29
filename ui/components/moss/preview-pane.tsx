"use client";

import { useState } from "react";
import { AlertCircle, AlertTriangle, CheckCircle2, ChevronDown, ChevronUp, FileX } from "lucide-react";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import type { CompileSummary, Diagnostic } from "@/lib/types";
import { PdfViewer, type PdfHighlight } from "./pdf-viewer";

type Props = {
  pdfUrl: string | null;
  result: CompileSummary | null;
  compiling: boolean;
  onJump: (file: string, line: number | null) => void;
  /** Where to scroll and flash in the PDF after "show in PDF". */
  highlight: PdfHighlight | null;
  /** Double-click in the PDF. */
  onInverse: (page: number, x: number, y: number) => void;
  follow: boolean;
  onFollowChange: (follow: boolean) => void;
  /** The Project folder name, for Save PDF and Open PDF. */
  project?: string | null;
};

export function PreviewPane({
  pdfUrl,
  result,
  compiling,
  onJump,
  highlight,
  onInverse,
  follow,
  onFollowChange,
  project,
}: Props) {
  // The list starts closed and stays however the user leaves it; it never opens by itself.
  const [open, setOpen] = useState(false);
  const errors = result?.diagnostics.filter((d) => d.severity === "Error") ?? [];
  const warnings = result?.diagnostics.filter((d) => d.severity === "Warning") ?? [];
  const hasList = Boolean(result) && (result!.diagnostics.length > 0 || result!.timed_out);

  return (
    <div className="flex h-full flex-col">
      <div className="min-h-0 flex-1 bg-muted/30">
        {pdfUrl ? (
          <PdfViewer
            url={pdfUrl}
            highlight={highlight}
            onInverse={onInverse}
            follow={follow}
            onFollowChange={onFollowChange}
            project={project}
          />
        ) : (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-sm text-muted-foreground">
            <FileX className="size-8 opacity-50" />
            {compiling ? "Compiling…" : "No PDF yet. Press Compile (Ctrl+Enter)."}
          </div>
        )}
      </div>

      {open && hasList && (
        <ScrollArea className="h-[40%] shrink-0 border-t">
          <ul className="divide-y text-sm">
            {result?.diagnostics.map((d, i) => (
              <DiagnosticRow key={i} d={d} onJump={onJump} />
            ))}
            {result?.timed_out && (
              <li className="px-3 py-2 text-destructive">The compile timed out.</li>
            )}
          </ul>
        </ScrollArea>
      )}

      <button
        type="button"
        disabled={!hasList}
        aria-expanded={open && hasList}
        onClick={() => setOpen(!open)}
        className={cn(
          "flex h-9 shrink-0 items-center gap-4 border-t px-3 text-left text-xs",
          hasList && "hover:bg-accent",
        )}
      >
        {result ? (
          <>
            {result.success ? (
              <span className="flex items-center gap-1.5 text-emerald-400">
                <CheckCircle2 className="size-4" /> Compiled
              </span>
            ) : null}
            <span className={cn("flex items-center gap-1.5", errors.length > 0 ? "text-destructive" : "text-muted-foreground")}>
              <AlertCircle className="size-4" /> {errors.length} error{errors.length === 1 ? "" : "s"}
            </span>
            <span className={cn("flex items-center gap-1.5", warnings.length > 0 ? "text-warning" : "text-muted-foreground")}>
              <AlertTriangle className="size-4" /> {warnings.length} warning{warnings.length === 1 ? "" : "s"}
            </span>
            <span className="ml-auto text-muted-foreground">
              {result.engine} · {(result.duration_ms / 1000).toFixed(1)} s
            </span>
            {hasList &&
              (open ? (
                <ChevronDown className="size-4 text-muted-foreground" />
              ) : (
                <ChevronUp className="size-4 text-muted-foreground" />
              ))}
          </>
        ) : (
          <span className="text-muted-foreground">Diagnostics appear here after a compile.</span>
        )}
      </button>
    </div>
  );
}

function DiagnosticRow({
  d,
  onJump,
}: {
  d: Diagnostic;
  onJump: (file: string, line: number | null) => void;
}) {
  const Icon = d.severity === "Error" ? AlertCircle : AlertTriangle;
  const color = d.severity === "Error" ? "text-destructive" : "text-warning";
  const body = (
    <>
      <Icon className={`mt-0.5 size-4 shrink-0 ${color}`} />
      <span className="min-w-0">
        <span className="block break-words">{d.message}</span>
        {(d.file || d.line) && (
          <span className="font-mono text-xs text-muted-foreground">
            {d.file ?? ""}
            {d.line ? `:${d.line}` : ""}
          </span>
        )}
      </span>
    </>
  );
  return (
    <li>
      {d.file ? (
        <button
          type="button"
          onClick={() => onJump(d.file!, d.line)}
          className="flex w-full items-start gap-2 px-3 py-2 text-left hover:bg-accent"
        >
          {body}
        </button>
      ) : (
        <div className="flex items-start gap-2 px-3 py-2">{body}</div>
      )}
    </li>
  );
}
