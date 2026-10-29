"use client";

import dynamic from "next/dynamic";
import { useEffect, useRef, useState } from "react";
import type { DiffOnMount } from "@monaco-editor/react";
import type * as MonacoApi from "monaco-editor";
import { ChevronDown, ChevronRight, Copy, FileMinus2, FilePlus2, FileText } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { configureMonacoLoader, MOSS_THEME, setupMonaco } from "@/lib/monaco-setup";
import type { ChangeKind, FileView } from "@/lib/types";
import { cn } from "@/lib/utils";

const MonacoDiff = dynamic(() => import("@monaco-editor/react").then((m) => m.DiffEditor), {
  ssr: false,
  loading: () => <Note>Loading diff…</Note>,
});

configureMonacoLoader();

/** Both sides of one file. `original` is the older (left) side. */
export type DiffPair = { original: FileView; modified: FileView };

type Props = {
  path: string;
  kind: ChangeKind;
  added: number;
  removed: number;
  binary: boolean;
  load: () => Promise<DiffPair>;
  defaultOpen?: boolean;
  /** Buttons at the right end of the header. */
  actions?: React.ReactNode;
  /** Re-open (and re-load) when this changes. */
  version?: string;
  /** Only the diff, for use under a row that already shows the path. */
  headerless?: boolean;
};

const MIN_HEIGHT = 72;
const MAX_HEIGHT = 640;

function languageFor(path: string) {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  if (["tex", "sty", "cls", "ltx", "def", "cfg"].includes(ext)) return "latex";
  if (ext === "bib") return "bibtex";
  if (ext === "json") return "json";
  if (ext === "md") return "markdown";
  return "plaintext";
}

function Note({ children }: { children: React.ReactNode }) {
  return <div className="px-4 py-6 text-center text-sm text-muted-foreground">{children}</div>;
}

const kb = (n: number) => (n < 1024 ? `${n} B` : `${(n / 1024).toFixed(1)} KB`);

export function FileDiff({ path, kind, added, removed, binary, load, defaultOpen = true, actions, version, headerless }: Props) {
  const [openState, setOpen] = useState(defaultOpen);
  const open = headerless || openState;
  const [pair, setPair] = useState<DiffPair | "error" | null>(null);
  const [height, setHeight] = useState(180);

  // The newest loader, without re-running the effect each render.
  const loadRef = useRef(load);
  useEffect(() => {
    loadRef.current = load;
  });

  useEffect(() => {
    if (!open || binary) return;
    let cancelled = false;
    loadRef.current().then(
      (p) => !cancelled && setPair(p),
      () => !cancelled && setPair("error"),
    );
    return () => {
      cancelled = true;
    };
  }, [open, binary, version]);

  // The diff editor and its two text models are disposed separately. Disposing a model while the
  // widget still shows it throws "TextModel got disposed before DiffEditorWidget model got reset",
  // so the models are kept by the wrapper and released here, after the widget lets go of them.
  const diffRef = useRef<MonacoApi.editor.IStandaloneDiffEditor | null>(null);
  useEffect(() => {
    return () => {
      const editor = diffRef.current;
      diffRef.current = null;
      const model = editor?.getModel();
      if (!editor || !model) return;
      try {
        editor.setModel(null);
      } catch {
        // already torn down
      }
      setTimeout(() => {
        model.original.dispose();
        model.modified.dispose();
      }, 0);
    };
  }, []);

  const onMount: DiffOnMount = (editor) => {
    diffRef.current = editor;
    const original = editor.getOriginalEditor();
    const modified = editor.getModifiedEditor();
    const fit = () => {
      const h = Math.max(original.getContentHeight(), modified.getContentHeight());
      setHeight(Math.min(Math.max(h, MIN_HEIGHT), MAX_HEIGHT));
    };
    modified.onDidContentSizeChange(fit);
    original.onDidContentSizeChange(fit);
    editor.onDidUpdateDiff(fit);
    fit();
  };

  const Icon = kind === "added" ? FilePlus2 : kind === "deleted" ? FileMinus2 : FileText;
  const tone = kind === "added" ? "text-emerald-400" : kind === "deleted" ? "text-destructive" : "text-primary";

  return (
    <section className={headerless ? undefined : "border-b"} data-testid="file-diff" data-path={path}>
      <div
        className={cn(
          "sticky top-0 z-10 flex h-10 items-center gap-2 bg-background/95 px-3 backdrop-blur",
          headerless && "hidden",
        )}
      >
        <button
          type="button"
          onClick={() => setOpen(!open)}
          aria-expanded={open}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          {open ? (
            <ChevronDown className="size-4 shrink-0 text-muted-foreground" />
          ) : (
            <ChevronRight className="size-4 shrink-0 text-muted-foreground" />
          )}
          <Icon className={cn("size-4 shrink-0", tone)} />
          <span className="truncate font-mono text-[13px]" title={path}>
            {path}
          </span>
        </button>
        <Button
          variant="ghost"
          size="icon-xs"
          title="Copy path"
          aria-label="Copy path"
          onClick={() => {
            void navigator.clipboard.writeText(path);
            toast.success("Path copied");
          }}
        >
          <Copy />
        </Button>
        {actions}
        {!binary && (
          <span className="shrink-0 font-mono text-xs tabular-nums">
            {removed > 0 && <span className="text-destructive">-{removed}</span>}
            {removed > 0 && added > 0 && " "}
            {added > 0 && <span className="text-emerald-400">+{added}</span>}
          </span>
        )}
      </div>

      {open &&
        (binary ? (
          <Note>This is a binary file, so its content is not shown.</Note>
        ) : pair === null ? (
          <Note>Loading…</Note>
        ) : pair === "error" ? (
          <Note>Could not load this file.</Note>
        ) : pair.original.binary || pair.modified.binary ? (
          <Note>
            Binary file ({pair.original.exists ? kb(pair.original.size) : "absent"} →{" "}
            {pair.modified.exists ? kb(pair.modified.size) : "absent"}).
          </Note>
        ) : pair.original.too_large || pair.modified.too_large ? (
          <Note>This file is too large to compare here.</Note>
        ) : (
          <div style={{ height }} data-testid="diff-body">
            <MonacoDiff
              original={pair.original.text ?? ""}
              modified={pair.modified.text ?? ""}
              language={languageFor(path)}
              theme={MOSS_THEME}
              keepCurrentOriginalModel
              keepCurrentModifiedModel
              beforeMount={(monaco) => setupMonaco(monaco)}
              onMount={onMount}
              options={{
                readOnly: true,
                originalEditable: false,
                renderSideBySide: true,
                // Always two panes: the inline view repeats every line number, which is noise.
                useInlineViewWhenSpaceIsLimited: false,
                renderSideBySideInlineBreakpoint: 0,
                enableSplitViewResizing: false,
                renderOverviewRuler: false,
                minimap: { enabled: false },
                scrollBeyondLastLine: false,
                automaticLayout: true,
                fontSize: 12.5,
                lineNumbersMinChars: 3,
                glyphMargin: false,
                folding: false,
                renderIndicators: true,
                diffAlgorithm: "advanced",
                hideUnchangedRegions: {
                  enabled: true,
                  contextLineCount: 3,
                  minimumLineCount: 4,
                  revealLineCount: 20,
                },
                scrollbar: { alwaysConsumeMouseWheel: false, useShadows: false },
              }}
            />
          </div>
        ))}
    </section>
  );
}
