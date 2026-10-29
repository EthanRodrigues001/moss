"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import {
  CaseSensitive,
  ChevronDown,
  ChevronRight,
  Loader2,
  Regex,
  Replace,
  Search,
  WholeWord,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { api } from "@/lib/moss-api";
import type { SearchFile, SearchLine, SearchOptions, SearchReport } from "@/lib/types";
import { cn } from "@/lib/utils";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The switches stay as the user left them for the rest of the session. */
const remembered = { caseSensitive: false, wholeWord: false, regex: false, include: "" };

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  project: string | null;
  /** Text to start with (the editor's selection). */
  initialQuery: string;
  /** Save unsaved edits so the search and the replace see what the editor shows. */
  beforeReplace: () => Promise<void>;
  onOpenHit: (path: string, line: number) => void;
  /** Files were rewritten: reload the open file, the tree and the History tab. */
  onReplaced: () => void;
};

/** Search (and replace) in every text file of the Project, in the style of the Add project palette. */
export function SearchDialog(props: Props) {
  return (
    <Dialog open={props.open} onOpenChange={props.onOpenChange}>
      <DialogContent
        showCloseButton={false}
        className="top-[10%] translate-y-0 gap-0 overflow-hidden p-0 sm:max-w-2xl [&>*]:min-w-0"
      >
        <DialogTitle className="sr-only">Search in project</DialogTitle>
        <DialogDescription className="sr-only">
          Find text in every file of the project and optionally replace it.
        </DialogDescription>
        {props.open && props.project && <Panel {...props} project={props.project} />}
      </DialogContent>
    </Dialog>
  );
}

function Panel({
  onOpenChange,
  project,
  initialQuery,
  beforeReplace,
  onOpenHit,
  onReplaced,
}: Props & { project: string }) {
  const [query, setQuery] = useState(initialQuery);
  const [replacement, setReplacement] = useState("");
  const [showReplace, setShowReplace] = useState(false);
  const [caseSensitive, setCaseSensitive] = useState(remembered.caseSensitive);
  const [wholeWord, setWholeWord] = useState(remembered.wholeWord);
  const [regex, setRegex] = useState(remembered.regex);
  const [include, setInclude] = useState(remembered.include);
  const [report, setReport] = useState<SearchReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  const [replacing, setReplacing] = useState(false);
  const [confirmAll, setConfirmAll] = useState(false);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [index, setIndex] = useState(0);
  const [nonce, setNonce] = useState(0);
  const sequence = useRef(0);

  const options: SearchOptions = useMemo(
    () => ({ query, case_sensitive: caseSensitive, whole_word: wholeWord, regex, include }),
    [query, caseSensitive, wholeWord, regex, include],
  );

  useEffect(() => {
    Object.assign(remembered, { caseSensitive, wholeWord, regex, include });
  }, [caseSensitive, wholeWord, regex, include]);

  // Search as the user types, after a short pause. Only the newest answer is kept.
  useEffect(() => {
    const id = ++sequence.current;
    if (!options.query) return;
    const timer = setTimeout(() => {
      setSearching(true);
      api
        .searchProject(project, options)
        .then((r) => {
          if (id !== sequence.current) return;
          setReport(r);
          setError(null);
          setIndex(0);
        })
        .catch((e) => {
          if (id !== sequence.current) return;
          setReport(null);
          setError(message(e));
        })
        .finally(() => {
          if (id === sequence.current) setSearching(false);
        });
    }, 200);
    return () => clearTimeout(timer);
  }, [project, options, nonce]);

  const hasQuery = query.length > 0;
  const shownReport = hasQuery ? report : null;
  const shownError = hasQuery ? error : null;

  // Every line that can be chosen with the arrow keys, in list order.
  const flat = useMemo(() => {
    const out: { path: string; line: number }[] = [];
    for (const f of shownReport?.files ?? []) {
      if (collapsed.has(f.path)) continue;
      for (const l of f.lines) out.push({ path: f.path, line: l.line });
    }
    return out;
  }, [shownReport, collapsed]);
  const active = Math.min(index, Math.max(0, flat.length - 1));

  function open(path: string, line: number) {
    onOpenChange(false);
    onOpenHit(path, line);
  }

  function toggle(path: string) {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (!next.delete(path)) next.add(path);
      return next;
    });
  }

  async function replace(paths?: string[]) {
    setReplacing(true);
    try {
      await beforeReplace();
      const done = await api.replaceInProject(project, options, replacement, paths);
      setConfirmAll(false);
      if (done.replacements === 0) {
        toast.message("Nothing was replaced");
      } else {
        toast.success(
          `Replaced ${done.replacements} match${done.replacements === 1 ? "" : "es"} in ${done.files.length} file${
            done.files.length === 1 ? "" : "s"
          }. Undo brings it back.`,
        );
        onReplaced();
      }
      setNonce((n) => n + 1);
    } catch (e) {
      toast.error(message(e));
    } finally {
      setReplacing(false);
    }
  }

  const total = shownReport?.total ?? 0;
  const fileCount = shownReport?.files.length ?? 0;
  const canReplace = hasQuery && total > 0 && !shownError && !replacing;

  return (
    <div
      onKeyDown={(e) => {
        if (e.key === "ArrowDown") {
          e.preventDefault();
          setIndex(Math.min(flat.length - 1, active + 1));
        } else if (e.key === "ArrowUp") {
          e.preventDefault();
          setIndex(Math.max(0, active - 1));
        } else if (e.key === "Enter" && e.target instanceof HTMLInputElement && e.target.dataset.role === "query") {
          const hit = flat[active];
          if (hit) {
            e.preventDefault();
            open(hit.path, hit.line);
          }
        }
      }}
    >
      <div className="flex items-center gap-3 border-b px-4 py-3">
        <Search className="size-4 shrink-0 text-muted-foreground" />
        <Input
          autoFocus
          data-role="query"
          value={query}
          placeholder="Search in project…"
          onFocus={(e) => e.currentTarget.select()}
          onChange={(e) => {
            setQuery(e.target.value);
            setConfirmAll(false);
          }}
          className="h-8 border-0 bg-transparent px-0 text-base shadow-none focus-visible:ring-0"
        />
        <div className="flex shrink-0 items-center gap-0.5">
          <Toggle label="Match case" on={caseSensitive} onClick={() => setCaseSensitive((v) => !v)}>
            <CaseSensitive className="size-4" />
          </Toggle>
          <Toggle label="Whole word" on={wholeWord} onClick={() => setWholeWord((v) => !v)}>
            <WholeWord className="size-4" />
          </Toggle>
          <Toggle label="Regular expression" on={regex} onClick={() => setRegex((v) => !v)}>
            <Regex className="size-4" />
          </Toggle>
          <Toggle label="Replace" on={showReplace} onClick={() => setShowReplace((v) => !v)}>
            <Replace className="size-4" />
          </Toggle>
        </div>
      </div>

      {showReplace && (
        <div className="flex items-center gap-3 border-b px-4 py-2">
          <Replace className="size-4 shrink-0 text-muted-foreground" />
          <Input
            value={replacement}
            placeholder={regex ? "Replace with… ($1 for a group)" : "Replace with…"}
            onChange={(e) => {
              setReplacement(e.target.value);
              setConfirmAll(false);
            }}
            className="h-8 border-0 bg-transparent px-0 text-sm shadow-none focus-visible:ring-0"
          />
          {confirmAll ? (
            <span className="flex shrink-0 items-center gap-1.5">
              <span className="text-xs text-muted-foreground">
                Replace {total} in {fileCount} file{fileCount === 1 ? "" : "s"}?
              </span>
              <Button size="xs" disabled={replacing} onClick={() => void replace()}>
                {replacing && <Loader2 data-icon="inline-start" className="animate-spin" />}
                Replace
              </Button>
              <Button size="xs" variant="ghost" onClick={() => setConfirmAll(false)}>
                Cancel
              </Button>
            </span>
          ) : (
            <Button size="xs" variant="outline" disabled={!canReplace} onClick={() => setConfirmAll(true)}>
              Replace all
            </Button>
          )}
        </div>
      )}

      <div className="flex items-center gap-3 border-b px-4 py-1.5">
        <span className="shrink-0 text-xs text-muted-foreground">Files</span>
        <Input
          value={include}
          placeholder="All text files. Or e.g. *.tex, chapters/**, !old"
          onChange={(e) => setInclude(e.target.value)}
          className="h-7 border-0 bg-transparent px-0 text-xs shadow-none focus-visible:ring-0"
        />
      </div>

      <div className="max-h-[52vh] min-h-24 overflow-x-hidden overflow-y-auto px-2 py-2">
        {!hasQuery && (
          <p className="px-3 py-8 text-center text-sm text-muted-foreground">
            Type to search every text file in this project. The files are saved first.
          </p>
        )}
        {shownError && <p className="px-3 py-6 text-center text-sm text-destructive">{shownError}</p>}
        {hasQuery && !shownError && shownReport && shownReport.files.length === 0 && !searching && (
          <p className="px-3 py-8 text-center text-sm text-muted-foreground">
            No matches in {shownReport.files_searched} file{shownReport.files_searched === 1 ? "" : "s"}.
          </p>
        )}
        {shownReport?.files.map((file) => (
          <FileGroup
            key={file.path}
            file={file}
            collapsed={collapsed.has(file.path)}
            activeLine={flat[active]?.path === file.path ? flat[active].line : null}
            showReplace={showReplace && canReplace}
            replacing={replacing}
            onToggle={() => toggle(file.path)}
            onOpen={(line) => open(file.path, line)}
            onHover={(line) => {
              const i = flat.findIndex((h) => h.path === file.path && h.line === line);
              if (i >= 0) setIndex(i);
            }}
            onReplaceFile={() => void replace([file.path])}
          />
        ))}
      </div>

      <div className="flex items-center gap-4 border-t px-4 py-2.5 text-xs text-muted-foreground">
        <Hint keys="↑ ↓" label="Navigate" />
        <Hint keys="Enter" label="Open" />
        <Hint keys="Esc" label="Close" />
        <span className="ml-auto flex items-center gap-1.5">
          {searching && <Loader2 className="size-3 animate-spin" />}
          {shownReport && !shownError
            ? `${total} match${total === 1 ? "" : "es"} in ${fileCount} file${fileCount === 1 ? "" : "s"}${
                shownReport.truncated ? " (list cut short)" : ""
              }`
            : ""}
        </span>
      </div>
    </div>
  );
}

function FileGroup({
  file,
  collapsed,
  activeLine,
  showReplace,
  replacing,
  onToggle,
  onOpen,
  onHover,
  onReplaceFile,
}: {
  file: SearchFile;
  collapsed: boolean;
  activeLine: number | null;
  showReplace: boolean;
  replacing: boolean;
  onToggle: () => void;
  onOpen: (line: number) => void;
  onHover: (line: number) => void;
  onReplaceFile: () => void;
}) {
  const activeRef = useRef<HTMLButtonElement | null>(null);
  useEffect(() => {
    if (activeLine !== null) activeRef.current?.scrollIntoView({ block: "nearest" });
  }, [activeLine]);

  return (
    <div className="pb-1">
      <div className="flex items-center gap-1 rounded-lg px-1 hover:bg-accent/50">
        <button
          type="button"
          onClick={onToggle}
          className="flex min-w-0 flex-1 items-center gap-2 py-1.5 pl-2 text-left"
        >
          {collapsed ? (
            <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />
          ) : (
            <ChevronDown className="size-3.5 shrink-0 text-muted-foreground" />
          )}
          <span className="min-w-0 truncate text-sm font-medium" title={file.path}>
            {file.path}
          </span>
          <span className="shrink-0 rounded-full bg-foreground/[0.08] px-1.5 text-[11px] text-muted-foreground">
            {file.count}
          </span>
        </button>
        {showReplace && (
          <Button size="xs" variant="ghost" disabled={replacing} onClick={onReplaceFile}>
            Replace in file
          </Button>
        )}
      </div>
      {!collapsed && (
        <ul>
          {file.lines.map((l) => (
            <li key={l.line}>
              <button
                type="button"
                ref={activeLine === l.line ? activeRef : undefined}
                onMouseMove={() => onHover(l.line)}
                onClick={() => onOpen(l.line)}
                className={cn(
                  "flex w-full items-baseline gap-3 rounded-lg py-1 pr-3 pl-8 text-left",
                  activeLine === l.line && "bg-accent",
                )}
              >
                <span className="w-8 shrink-0 text-right font-mono text-xs text-muted-foreground">{l.line}</span>
                <Snippet line={l} />
              </button>
            </li>
          ))}
          {file.lines_truncated && (
            <li className="py-1 pl-16 text-xs text-muted-foreground">
              More matches in this file than are listed ({file.count} in all).
            </li>
          )}
        </ul>
      )}
    </div>
  );
}

/** The line with every match marked. Ranges are character offsets, so count code points. */
function Snippet({ line }: { line: SearchLine }) {
  const chars = Array.from(line.text);
  const parts: React.ReactNode[] = [];
  let at = 0;
  line.ranges.forEach(([start, end], i) => {
    if (start > at) parts.push(chars.slice(at, start).join(""));
    parts.push(
      <mark key={i} className="rounded-sm bg-amber-400/30 text-foreground">
        {chars.slice(start, end).join("")}
      </mark>,
    );
    at = end;
  });
  if (at < chars.length) parts.push(chars.slice(at).join(""));
  return <span className="min-w-0 truncate font-mono text-xs whitespace-pre">{parts}</span>;
}

function Toggle({
  label,
  on,
  onClick,
  children,
}: {
  label: string;
  on: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={on}
      onClick={onClick}
      className={cn(
        "rounded-md p-1.5 text-muted-foreground transition-colors hover:text-foreground",
        on && "bg-accent text-foreground",
      )}
    >
      {children}
    </button>
  );
}

function Hint({ keys, label }: { keys: string; label: string }) {
  return (
    <span className="flex items-center gap-1.5">
      <kbd className="rounded-md bg-foreground/[0.08] px-1.5 py-0.5 font-sans text-[11px]">{keys}</kbd>
      {label}
    </span>
  );
}
