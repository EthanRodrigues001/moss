"use client";

import { useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  ArrowDownToLine,
  ArrowRight,
  ArrowUpFromLine,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  GitMerge,
  Loader2,
  Minus,
  Plus,
  RefreshCw,
  TriangleAlert,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { api } from "@/lib/moss-api";
import { choiceFrom, type Draft, hasConflictMarkers, openCount } from "@/lib/merge";
import { plural } from "@/lib/time";
import type {
  ApplyRequest,
  Applied,
  Conflict,
  Resolution,
  SyncItem,
  SyncMode,
  SyncOutcome,
  SyncPreview,
} from "@/lib/types";
import { cn } from "@/lib/utils";
import { ConflictResolver } from "./conflict-resolver";
import { FileDiff } from "./file-diff";

type Props = {
  mode: SyncMode;
  /** The Project's folder name, and its name for the title. */
  project: string;
  projectName: string;
  onClose: () => void;
  /** Files, the log or the link changed: reload what depends on them. */
  onApplied: () => void;
};

type Phase = "loading" | "review" | "conflicts" | "applying" | "stale" | "done" | "error";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

const TITLE: Record<SyncMode, string> = {
  pull: "Pull from Overleaf",
  push: "Push to Overleaf",
  sync: "Sync with Overleaf",
};
const GO: Record<SyncMode, string> = { pull: "Pull", push: "Push", sync: "Sync" };

/**
 * The Review: what a Pull, Push or Sync will do, file by file, before anything changes. It owns
 * the whole flow (Review, Conflicts, Apply, Result) so the Project's files only change when the
 * user presses the final button.
 */
export function SyncReviewDialog({ mode, project, projectName, onClose, onApplied }: Props) {
  const [phase, setPhase] = useState<Phase>("loading");
  const [attempt, setAttempt] = useState(0);
  const [preview, setPreview] = useState<SyncPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [skip, setSkip] = useState<Set<string>>(new Set());
  const [drafts, setDrafts] = useState<Record<string, Draft>>({});
  const [activeConflict, setActiveConflict] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [allowMass, setAllowMass] = useState(false);
  const [allowMarkers, setAllowMarkers] = useState(false);
  const [outcome, setOutcome] = useState<SyncOutcome | null>(null);
  const [staleSide, setStaleSide] = useState<"overleaf" | "here">("overleaf");

  useEffect(() => {
    let cancelled = false;
    api.syncPreview(project, mode).then(
      (p) => {
        if (cancelled) return;
        setPreview(p);
        setPhase("review");
      },
      (e) => {
        if (cancelled) return;
        setError(message(e));
        setPhase("error");
      },
    );
    return () => {
      cancelled = true;
    };
  }, [project, mode, attempt]);

  const reReview = () => {
    setPreview(null);
    setSkip(new Set());
    setDrafts({});
    setPhase("loading");
    setAttempt((a) => a + 1);
  };

  const applicable = useMemo(() => (preview?.items ?? []).filter((i) => i.applies), [preview]);
  const leftAlone = useMemo(() => (preview?.items ?? []).filter((i) => !i.applies), [preview]);
  const conflicts: Conflict[] = useMemo(
    () => (preview?.conflicts ?? []).filter((c) => !skip.has(c.path)),
    [preview, skip],
  );
  // Conflicts are counted separately: they are answered in the next step.
  const included = applicable.filter((i) => !skip.has(i.path) && i.op !== "conflict");
  const open = conflicts.reduce((n, c) => n + openCount(c, drafts[c.path]), 0);
  const unresolved = conflicts.filter((c) => choiceFrom(drafts[c.path]) === null);
  const markers = conflicts.some((c) => {
    const d = drafts[c.path];
    return d?.kind === "text" && hasConflictMarkers(d.text);
  });
  const massBlocked = Boolean(preview?.mass_delete) && !allowMass;

  const toggle = (path: string) =>
    setSkip((prev) => {
      const next = new Set(prev);
      if (!next.delete(path)) next.add(path);
      return next;
    });

  async function apply() {
    if (!preview) return;
    const resolutions: Resolution[] = conflicts.flatMap((c) => {
      const choice = choiceFrom(drafts[c.path]);
      return choice ? [{ path: c.path, choice }] : [];
    });
    const request: ApplyRequest = {
      mode,
      token: preview.token,
      resolutions,
      allow_mass_delete: allowMass,
      allow_markers: allowMarkers,
      skip: [...skip],
    };
    setPhase("applying");
    try {
      const result: Applied = await api.syncApply(project, request);
      if (result.result === "stale") {
        setStaleSide(result.side);
        setPhase("stale");
        return;
      }
      setOutcome(result.outcome);
      setPhase("done");
      onApplied();
    } catch (e) {
      setError(message(e));
      setPhase("error");
      toast.error(message(e));
    }
  }

  const next = () => {
    if (conflicts.length > 0) {
      setActiveConflict((cur) => cur ?? conflicts[0].path);
      setPhase("conflicts");
    } else void apply();
  };

  const busy = phase === "loading" || phase === "applying";
  const nothing = preview && included.length === 0 && conflicts.length === 0;

  return (
    <Dialog open onOpenChange={(o) => !o && !busy && onClose()}>
      <DialogContent className="flex h-[min(86vh,760px)] flex-col gap-0 p-0 sm:max-w-4xl" data-testid="sync-review">
        <DialogHeader className="border-b px-5 py-4">
          <DialogTitle className="flex items-center gap-2">
            {mode === "pull" ? <ArrowDownToLine className="size-4" /> : mode === "push" ? <ArrowUpFromLine className="size-4" /> : <RefreshCw className="size-4" />}
            {TITLE[mode]}
          </DialogTitle>
          <DialogDescription>
            {projectName}. Nothing changes until you press {phase === "conflicts" ? "Apply" : GO[mode]} at the end.
          </DialogDescription>
        </DialogHeader>

        <div className="min-h-0 flex-1 overflow-hidden">
          {phase === "loading" && (
            <Centered>
              <Loader2 className="size-5 animate-spin" />
              Checking what changed on both sides…
            </Centered>
          )}
          {phase === "applying" && (
            <Centered>
              <Loader2 className="size-5 animate-spin" />
              Applying…
            </Centered>
          )}
          {phase === "error" && (
            <Centered>
              <TriangleAlert className="size-6 text-destructive" />
              <p className="max-w-md text-center text-destructive">{error}</p>
            </Centered>
          )}
          {phase === "stale" && (
            <Centered>
              <AlertTriangle className="size-6 text-amber-400" />
              <p className="max-w-md text-center">
                {staleSide === "overleaf"
                  ? "Overleaf changed while you were reviewing."
                  : "Your files changed while you were reviewing."}{" "}
                Nothing was written. Review again to see the new picture.
              </p>
              <Button onClick={reReview}>
                <RefreshCw data-icon="inline-start" />
                Review again
              </Button>
            </Centered>
          )}

          {phase === "done" && outcome && <Result outcome={outcome} />}

          {phase === "review" && preview && (
            <div className="h-full overflow-auto">
              {preview.blocked && (
                <Notice tone="error">
                  <TriangleAlert className="size-4 shrink-0" />
                  {preview.blocked}
                </Notice>
              )}
              {preview.warnings.map((w) => (
                <Notice key={w} tone="warn">
                  <AlertTriangle className="size-4 shrink-0" />
                  {w}
                </Notice>
              ))}
              {preview.mass_delete && (
                <Notice tone="error">
                  <TriangleAlert className="size-4 shrink-0" />
                  <span className="flex-1">
                    This will delete {preview.mass_delete.count} of {preview.mass_delete.total} files{" "}
                    {preview.mass_delete.side === "here" ? "on this PC" : "on Overleaf"}.
                  </span>
                  <label className="flex shrink-0 items-center gap-2 font-medium">
                    <input type="checkbox" className="size-4 accent-primary" checked={allowMass} onChange={(e) => setAllowMass(e.target.checked)} />
                    I understand
                  </label>
                </Notice>
              )}

              {nothing && !preview.blocked && (
                <Centered>
                  <CheckCircle2 className="size-6 text-emerald-400" />
                  <p>Everything is up to date. Nothing to {mode === "sync" ? "sync" : mode}.</p>
                </Centered>
              )}

              <Summary preview={preview} included={included.length} />

              <ItemSection title="Coming from Overleaf" items={included.filter((i) => i.direction === "pull")} {...{ skip, toggle, expanded, setExpanded, project }} />
              <ItemSection title="Going to Overleaf" items={included.filter((i) => i.direction === "push")} {...{ skip, toggle, expanded, setExpanded, project }} />
              <ItemSection title="Changed on both sides, combined automatically" items={included.filter((i) => i.op === "merge")} {...{ skip, toggle, expanded, setExpanded, project }} />
              <ItemSection title="Need your decision" items={applicable.filter((i) => i.op === "conflict")} {...{ skip, toggle, expanded, setExpanded, project }} conflictRows />
              <ItemSection title="Left alone this time" items={leftAlone} {...{ skip, toggle, expanded, setExpanded, project }} muted />
              {applicable.filter((i) => skip.has(i.path) && i.op !== "conflict").length > 0 && (
                <ItemSection title="You unticked" items={applicable.filter((i) => skip.has(i.path) && i.op !== "conflict")} {...{ skip, toggle, expanded, setExpanded, project }} />
              )}

              {preview.needs_attention.length > 0 && (
                <section className="border-t px-5 py-3">
                  <h4 className="text-xs font-semibold tracking-wider text-muted-foreground uppercase">
                    Needs attention · skipped
                  </h4>
                  <ul className="mt-2 space-y-1 text-xs text-muted-foreground">
                    {preview.needs_attention.map((s) => (
                      <li key={s.path}>
                        <span className="font-mono text-foreground">{s.path}</span> — {s.reason}
                      </li>
                    ))}
                  </ul>
                </section>
              )}
            </div>
          )}

          {phase === "conflicts" && preview && (
            <div className="grid h-full grid-cols-[220px_minmax(0,1fr)]">
              <ul className="overflow-auto border-r py-2">
                {conflicts.map((c) => {
                  const left = openCount(c, drafts[c.path]);
                  return (
                    <li key={c.path}>
                      <button
                        type="button"
                        onClick={() => setActiveConflict(c.path)}
                        className={cn(
                          "flex w-full items-start gap-2 px-3 py-2 text-left text-xs hover:bg-accent",
                          activeConflict === c.path && "bg-accent",
                        )}
                      >
                        {left === 0 ? (
                          <CheckCircle2 className="mt-0.5 size-3.5 shrink-0 text-emerald-400" />
                        ) : (
                          <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-amber-400" />
                        )}
                        <span className="min-w-0">
                          <span className="block truncate font-mono">{c.path.split("/").pop()}</span>
                          <span className="text-muted-foreground">
                            {left === 0 ? "answered" : `${left} to answer`}
                          </span>
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ul>
              <div className="min-w-0">
                {conflicts
                  .filter((c) => c.path === activeConflict)
                  .map((c) => (
                    <ConflictResolver
                      key={c.path}
                      conflict={c}
                      draft={drafts[c.path]}
                      onChange={(d) =>
                        setDrafts((prev) => {
                          const copy = { ...prev };
                          if (d) copy[c.path] = d;
                          else delete copy[c.path];
                          return copy;
                        })
                      }
                    />
                  ))}
              </div>
            </div>
          )}
        </div>

        <div className="flex items-center gap-3 border-t px-5 py-3">
          {phase === "review" && preview && (
            <>
              <p className="text-xs text-muted-foreground">
                {nothing ? "" : `${plural(included.length + conflicts.length, "file")} will change`}
                {conflicts.length > 0 && ` · ${plural(conflicts.length, "conflict")} to answer`}
              </p>
              <div className="ml-auto flex gap-2">
                <Button variant="ghost" onClick={onClose}>
                  Cancel
                </Button>
                {!nothing && (
                  <Button onClick={next} disabled={Boolean(preview.blocked) || massBlocked}>
                    {conflicts.length > 0 ? (
                      <>
                        Resolve {plural(conflicts.length, "conflict")}
                        <ArrowRight data-icon="inline-end" />
                      </>
                    ) : (
                      <>
                        <GitMerge data-icon="inline-start" />
                        {GO[mode]} {plural(included.length, "file")}
                      </>
                    )}
                  </Button>
                )}
              </div>
            </>
          )}
          {phase === "conflicts" && (
            <>
              <p className="text-xs text-muted-foreground">
                {open === 0 ? "Every conflict has an answer." : `${plural(open, "decision")} left.`}
              </p>
              {markers && (
                <label className="flex items-center gap-2 text-xs text-amber-300">
                  <input type="checkbox" className="size-4 accent-primary" checked={allowMarkers} onChange={(e) => setAllowMarkers(e.target.checked)} />
                  Keep the conflict markers
                </label>
              )}
              <div className="ml-auto flex gap-2">
                <Button variant="ghost" onClick={() => setPhase("review")}>
                  Back
                </Button>
                <Button onClick={() => void apply()} disabled={unresolved.length > 0 || (markers && !allowMarkers)}>
                  <GitMerge data-icon="inline-start" />
                  Apply
                </Button>
              </div>
            </>
          )}
          {(phase === "done" || phase === "error") && (
            <Button className="ml-auto" onClick={onClose}>
              Close
            </Button>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function Centered({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-full min-h-48 flex-col items-center justify-center gap-3 p-6 text-sm text-muted-foreground">
      {children}
    </div>
  );
}

function Notice({ tone, children }: { tone: "warn" | "error"; children: React.ReactNode }) {
  return (
    <div
      className={cn(
        "flex items-center gap-2 border-b px-5 py-2.5 text-xs",
        tone === "warn" ? "bg-amber-500/10 text-amber-300" : "bg-destructive/10 text-destructive",
      )}
    >
      {children}
    </div>
  );
}

function Summary({ preview, included }: { preview: SyncPreview; included: number }) {
  const s = preview.summary;
  const chip = (label: string, n: number, className: string) =>
    n > 0 ? (
      <span key={label} className={cn("rounded-md border px-2 py-0.5 text-xs font-medium", className)}>
        {label} {n}
      </span>
    ) : null;
  if (included === 0 && s.conflict === 0) return null;
  return (
    <div className="flex flex-wrap items-center gap-2 border-b px-5 py-3">
      {chip("↓ Pull", s.pull, "text-emerald-400")}
      {chip("↑ Push", s.push, "text-sky-400")}
      {chip("Merge", s.merge, "text-amber-400")}
      {chip("Conflict", s.conflict, "text-destructive")}
      {chip("Delete here", s.delete_here, "text-destructive")}
      {chip("Delete on Overleaf", s.delete_there, "text-destructive")}
    </div>
  );
}

type SectionProps = {
  title: string;
  items: SyncItem[];
  skip: Set<string>;
  toggle: (path: string) => void;
  expanded: string | null;
  setExpanded: (path: string | null) => void;
  project: string;
  muted?: boolean;
  conflictRows?: boolean;
};

function ItemSection({ title, items, skip, toggle, expanded, setExpanded, project, muted, conflictRows }: SectionProps) {
  if (items.length === 0) return null;
  return (
    <section className="border-b">
      <h4 className="px-5 pt-3 pb-1 text-xs font-semibold tracking-wider text-muted-foreground uppercase">
        {title} · {items.length}
      </h4>
      <ul>
        {items.map((item) => {
          const isOpen = expanded === item.path;
          const unticked = skip.has(item.path);
          const canDiff = !conflictRows && !item.binary && item.op !== "rename" && item.applies;
          return (
            <li key={`${item.path}:${item.op}`} data-testid="sync-item" data-op={item.op}>
              <div className={cn("flex items-center gap-2 px-5 py-1.5 text-sm", muted && "opacity-60", unticked && "opacity-60")}>
                {item.applies ? (
                  <input
                    type="checkbox"
                    className="size-4 accent-primary"
                    aria-label={`Include ${item.path}`}
                    checked={!unticked}
                    onChange={() => toggle(item.path)}
                  />
                ) : (
                  <span className="size-4" />
                )}
                <OpIcon op={item.op} />
                <button
                  type="button"
                  disabled={!canDiff}
                  onClick={() => setExpanded(isOpen ? null : item.path)}
                  className={cn("flex min-w-0 flex-1 items-center gap-1.5 text-left", canDiff && "hover:text-foreground")}
                  aria-expanded={isOpen}
                >
                  {canDiff &&
                    (isOpen ? <ChevronDown className="size-3.5 shrink-0" /> : <ChevronRight className="size-3.5 shrink-0" />)}
                  <span className="truncate font-mono text-[13px]">
                    {item.from ? `${item.from} → ${item.path}` : item.path}
                  </span>
                </button>
                {item.note && <span className="shrink-0 text-xs text-muted-foreground">{item.note}</span>}
                {item.binary ? (
                  <span className="shrink-0 text-xs text-muted-foreground">binary</span>
                ) : (
                  (item.lines.added > 0 || item.lines.removed > 0) && (
                    <span className="shrink-0 font-mono text-xs tabular-nums">
                      {item.lines.removed > 0 && <span className="text-destructive">-{item.lines.removed} </span>}
                      {item.lines.added > 0 && <span className="text-emerald-400">+{item.lines.added}</span>}
                    </span>
                  )
                )}
              </div>
              {isOpen && canDiff && (
                <div className="mx-5 mb-2 overflow-hidden rounded-lg border">
                  <FileDiff
                    headerless
                    path={item.path}
                    kind={item.op === "add" ? "added" : item.op === "delete" ? "deleted" : "modified"}
                    added={item.lines.added}
                    removed={item.lines.removed}
                    binary={item.binary}
                    load={async () => {
                      const files = await api.syncFile(project, item.path);
                      // Pull: what is here now becomes what Overleaf has. Push: the reverse.
                      return item.direction === "push"
                        ? { original: files.theirs, modified: files.mine }
                        : { original: files.mine, modified: files.theirs };
                    }}
                  />
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function OpIcon({ op }: { op: SyncItem["op"] }) {
  const cls = "size-4 shrink-0";
  switch (op) {
    case "add":
      return <Plus className={cn(cls, "text-emerald-400")} />;
    case "delete":
      return <Minus className={cn(cls, "text-destructive")} />;
    case "rename":
      return <ArrowRight className={cn(cls, "text-sky-400")} />;
    case "merge":
      return <GitMerge className={cn(cls, "text-amber-400")} />;
    case "conflict":
      return <AlertTriangle className={cn(cls, "text-amber-400")} />;
    default:
      return <span className={cn(cls, "flex items-center justify-center")}><span className="size-1.5 rounded-full bg-primary" /></span>;
  }
}

function Result({ outcome }: { outcome: SyncOutcome }) {
  const lines: string[] = [];
  if (outcome.pulled) lines.push(`${plural(outcome.pulled, "file")} pulled from Overleaf`);
  if (outcome.merged) lines.push(`${plural(outcome.merged, "file")} merged`);
  if (outcome.resolved) lines.push(`${plural(outcome.resolved, "conflict")} resolved`);
  if (outcome.pushed) lines.push(`${plural(outcome.pushed, "file")} sent to Overleaf`);
  if (outcome.left_alone) lines.push(`${plural(outcome.left_alone, "file")} left alone`);
  return (
    <div className="h-full overflow-auto p-6" data-testid="sync-result">
      <div className="mx-auto flex max-w-lg flex-col items-center gap-3 text-center">
        {outcome.complete ? (
          <CheckCircle2 className="size-8 text-emerald-400" />
        ) : (
          <AlertTriangle className="size-8 text-amber-400" />
        )}
        <p className="text-base font-semibold">
          {outcome.complete ? "Done" : outcome.aborted ? "Stopped early" : "Done, with problems"}
        </p>
        <ul className="space-y-1 text-sm text-muted-foreground">
          {lines.length === 0 && <li>Nothing needed to change.</li>}
          {lines.map((l) => (
            <li key={l}>{l}</li>
          ))}
        </ul>
        {outcome.aborted && <p className="text-sm text-amber-300">{outcome.aborted}</p>}
        {outcome.failures.length > 0 && (
          <div className="w-full rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-left text-xs">
            <p className="mb-1 font-semibold text-destructive">
              {plural(outcome.failures.length, "file")} could not be sent. Sync again to retry.
            </p>
            <ul className="space-y-1">
              {outcome.failures.map((f) => (
                <li key={f.path}>
                  <span className="font-mono">{f.path}</span>: {f.error}
                </li>
              ))}
            </ul>
          </div>
        )}
        <p className="text-xs text-muted-foreground">
          Everything is in the History tab. The Undo button in the header steps back over a pull or merge.
        </p>
      </div>
    </div>
  );
}
