"use client";

import dynamic from "next/dynamic";
import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import type { Editor as TiptapEditor } from "@tiptap/react";
import type { Monaco, OnMount } from "@monaco-editor/react";
import type * as MonacoApi from "monaco-editor";
import { Button } from "@/components/ui/button";
import { lintLatex } from "@/lib/latex-lint";
import { findEquationAtOffset, type EquationSelection } from "@/lib/latex-math";
import { configureMonacoLoader, MOSS_THEME, setupMonaco } from "@/lib/monaco-setup";
import type { Diagnostic, FileKind } from "@/lib/types";
import { useDialogs } from "./dialogs";
import { EditorToolbar, type EditActions, type HeadingKind } from "./editor-toolbar";

const MonacoEditor = dynamic(() => import("@monaco-editor/react"), {
  ssr: false,
  loading: () => <Placeholder>Loading editor…</Placeholder>,
});
const LatexVisualEditor = dynamic(
  () => import("./latex-visual-editor").then((m) => m.LatexVisualEditor),
  { ssr: false, loading: () => <Placeholder>Loading visual editor…</Placeholder> },
);
const EquationEditor = dynamic(() => import("./equation-editor").then((m) => m.EquationEditor), {
  ssr: false,
});

configureMonacoLoader();

export type EditorHandle = {
  /** Put the caret at the start of a 1-based line and scroll it into view. */
  goToLine: (line: number) => void;
  /** The text selected in the source editor (empty when nothing is, or in the visual editor). */
  getSelection: () => string;
};

type Props = {
  path: string | null;
  kind: FileKind | null;
  editable: boolean;
  value: string;
  /** Errors and warnings from the last compile; those for this file are marked on their lines. */
  problems?: Diagnostic[];
  onChange: (value: string) => void;
  /** An edit made in the visual editor (it rewrites LaTeX, so the app takes a restore point first). */
  onVisualChange?: (value: string) => void;
  onCompile: () => void;
  /** Show the caret's line in the PDF (SyncTeX forward search). */
  onShowInPdf: (line: number, column: number) => void;
  /** The caret moved (typing, clicking or arrow keys). */
  onCursor: (line: number, column: number) => void;
};

const NO_PROBLEMS: Diagnostic[] = [];

const TABLE = [
  "\\begin{table}[ht]",
  "  \\centering",
  "  \\begin{tabular}{|c|c|}",
  "    \\hline",
  "    A & B \\\\",
  "    \\hline",
  "    1 & 2 \\\\",
  "    \\hline",
  "  \\end{tabular}",
  "  \\caption{Caption}",
  "  \\label{tab:label}",
  "\\end{table}",
].join("\n");

function Placeholder({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
      {children}
    </div>
  );
}

export const EditorPane = forwardRef<EditorHandle, Props>(function EditorPane(
  { path, kind, editable, value, problems = NO_PROBLEMS, onChange, onVisualChange, onCompile, onShowInPdf, onCursor },
  ref,
) {
  const editorRef = useRef<MonacoApi.editor.IStandaloneCodeEditor | null>(null);
  const decorationsRef = useRef<MonacoApi.editor.IEditorDecorationsCollection | null>(null);
  const monacoRef = useRef<Monaco | null>(null);
  const compileRef = useRef(onCompile);
  const showRef = useRef(onShowInPdf);
  const cursorRef = useRef(onCursor);
  const [mounted, setMounted] = useState(false);
  const [mode, setMode] = useState<"source" | "visual">("source");
  const [equationOpen, setEquationOpen] = useState(false);
  const [equation, setEquation] = useState("x^2 + y^2 = z^2");
  const [atCursor, setAtCursor] = useState<EquationSelection | null>(null);
  const [sourceHeading, setSourceHeading] = useState<HeadingKind>("normal");
  const [visualEditor, setVisualEditor] = useState<TiptapEditor | null>(null);
  const [, redrawToolbar] = useState(0);
  const tick = useCallback(() => redrawToolbar((n) => n + 1), []);
  const { prompt } = useDialogs();

  useEffect(() => {
    compileRef.current = onCompile;
    showRef.current = onShowInPdf;
    cursorRef.current = onCursor;
  });

  // Monaco cancels its own pending work when a model is swapped; the rejection is expected noise.
  useEffect(() => {
    const quiet = (e: PromiseRejectionEvent) => {
      const r = e.reason as { name?: string; message?: string } | string | undefined;
      if (r === "Canceled" || (typeof r === "object" && (r?.name === "Canceled" || r?.message === "Canceled"))) {
        e.preventDefault();
      }
    };
    window.addEventListener("unhandledrejection", quiet);
    return () => window.removeEventListener("unhandledrejection", quiet);
  }, []);

  useImperativeHandle(ref, () => ({
    goToLine(line) {
      const editor = editorRef.current;
      if (!editor) return;
      setMode("source");
      editor.revealLineInCenter(line);
      editor.setPosition({ lineNumber: line, column: 1 });
      editor.setSelection({
        startLineNumber: line,
        startColumn: 1,
        endLineNumber: line,
        endColumn: editor.getModel()?.getLineMaxColumn(line) ?? 1,
      });
      editor.focus();
    },
    getSelection() {
      const editor = editorRef.current;
      const range = editor?.getSelection();
      if (!editor || !range || range.isEmpty()) return "";
      const text = editor.getModel()?.getValueInRange(range) ?? "";
      return /[\r\n]/.test(text) ? "" : text;
    },
  }));

  const isTex = kind === "tex";

  // Underline obvious mistakes (unmatched environments, broken math) as the user types.
  useEffect(() => {
    const monaco = monacoRef.current;
    const model = editorRef.current?.getModel();
    if (!monaco || !model) return;
    const markers = isTex
      ? lintLatex(value).map((d) => ({
          message: d.message,
          startLineNumber: d.startLineNumber,
          startColumn: d.startColumn,
          endLineNumber: d.endLineNumber,
          endColumn: d.endColumn,
          severity:
            d.severity === "error"
              ? monaco.MarkerSeverity.Error
              : d.severity === "warning"
                ? monaco.MarkerSeverity.Warning
                : monaco.MarkerSeverity.Info,
        }))
      : [];
    // Errors and warnings from the last compile, on the line LaTeX reported.
    const here = (path ?? "").replace(/^\.\//, "");
    for (const d of problems) {
      const file = d.file?.replace(/\\/g, "/").replace(/^\.\//, "");
      if (!d.line || !file || !(file === here || here.endsWith(`/${file}`) || file.endsWith(`/${here}`))) continue;
      if (d.line > model.getLineCount()) continue;
      markers.push({
        message: `LaTeX: ${d.message}`,
        startLineNumber: d.line,
        startColumn: 1,
        endLineNumber: d.line,
        endColumn: model.getLineMaxColumn(d.line),
        severity: d.severity === "Error" ? monaco.MarkerSeverity.Error : monaco.MarkerSeverity.Warning,
      });
    }
    monaco.editor.setModelMarkers(model, "moss-lint", markers);

    // A dot in the gutter whose tooltip is the message, so a mistake can be read without
    // hovering the small underline.
    const lines = new Map<number, { message: string; error: boolean }>();
    for (const m of markers) {
      const error = m.severity === monaco.MarkerSeverity.Error;
      const seen = lines.get(m.startLineNumber);
      if (!seen) lines.set(m.startLineNumber, { message: m.message, error });
      else lines.set(m.startLineNumber, { message: `${seen.message}\n\n${m.message}`, error: seen.error || error });
    }
    const decorations = [...lines].map(([line, d]) => ({
      range: new monaco.Range(line, 1, line, 1),
      options: {
        glyphMarginClassName: d.error ? "moss-gutter-error" : "moss-gutter-warning",
        glyphMarginHoverMessage: { value: d.message },
      },
    }));
    decorationsRef.current?.clear();
    decorationsRef.current = editorRef.current?.createDecorationsCollection(decorations) ?? null;
  }, [value, isTex, path, mounted, mode, problems]);

  const trackCursor = useCallback(() => {
    const editor = editorRef.current;
    const model = editor?.getModel();
    const position = editor?.getPosition();
    if (!model || !position) return setAtCursor(null);
    const heading = /^\s*\\(section|subsection|subsubsection|paragraph|subparagraph)\*?\{/.exec(
      model.getLineContent(position.lineNumber),
    );
    setSourceHeading(heading ? (heading[1] as HeadingKind) : "normal");
    setAtCursor(findEquationAtOffset(model.getValue(), model.getOffsetAt(position)));
  }, []);

  const onMount: OnMount = useCallback(
    (editor, monaco) => {
      editorRef.current = editor;
      monacoRef.current = monaco;
      setMounted(true);
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter, () => compileRef.current());
      editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyMod.Shift | monaco.KeyCode.KeyJ, () => {
        const at = editor.getPosition();
        if (at) showRef.current(at.lineNumber, at.column);
      });
      editor.onDidChangeCursorPosition((e) => {
        trackCursor();
        cursorRef.current(e.position.lineNumber, e.position.column);
      });
      trackCursor();
    },
    [trackCursor],
  );

  function editRange(start: number, end: number, text: string) {
    const editor = editorRef.current;
    const model = editor?.getModel();
    if (!editor || !model) return;
    const from = model.getPositionAt(start);
    const to = model.getPositionAt(end);
    editor.executeEdits("moss-equation", [
      {
        range: {
          startLineNumber: from.lineNumber,
          startColumn: from.column,
          endLineNumber: to.lineNumber,
          endColumn: to.column,
        },
        text,
        forceMoveMarkers: true,
      },
    ]);
    editor.focus();
  }

  function insertEquation(display: boolean) {
    const editor = editorRef.current;
    const model = editor?.getModel();
    const position = editor?.getPosition();
    if (!editor || !model || !position) return;
    const offset = model.getOffsetAt(position);
    editRange(offset, offset, display ? `\\[\n${equation}\n\\]` : `$${equation}$`);
  }

  function replaceEquation() {
    if (!atCursor) return;
    const body = atCursor.displayMode ? `\n${equation}\n` : equation;
    editRange(atCursor.startOffset, atCursor.endOffset, `${atCursor.open}${body}${atCursor.close}`);
  }

  function loadEquation() {
    if (atCursor) setEquation(atCursor.latex);
  }

  const labels = useMemo(() => {
    const found = new Set<string>();
    for (const m of value.matchAll(/\\label\{([^}\s]+)\}/g)) found.add(m[1]);
    return [...found].slice(0, 40);
  }, [value]);

  /** Replace the selection in the source and select `len` characters starting `from` into the new text. */
  function putText(text: string, from = text.length, len = 0) {
    const editor = editorRef.current;
    const model = editor?.getModel();
    const sel = editor?.getSelection();
    if (!editor || !model || !sel) return;
    const start = model.getOffsetAt({ lineNumber: sel.startLineNumber, column: sel.startColumn });
    editor.executeEdits("moss-toolbar", [{ range: sel, text, forceMoveMarkers: true }]);
    const a = model.getPositionAt(start + from);
    const b = model.getPositionAt(start + from + len);
    editor.setSelection({
      startLineNumber: a.lineNumber,
      startColumn: a.column,
      endLineNumber: b.lineNumber,
      endColumn: b.column,
    });
    editor.focus();
  }

  /** Wrap the selection, or insert the pair with a placeholder selected so typing replaces it. */
  function wrap(before: string, after: string, placeholder: string) {
    const editor = editorRef.current;
    const sel = editor?.getSelection();
    const picked = sel && !sel.isEmpty() ? (editor?.getModel()?.getValueInRange(sel) ?? "") : "";
    if (picked) putText(before + picked + after);
    else putText(before + placeholder + after, before.length, placeholder.length);
  }

  function setSourceHeadingKind(kind: HeadingKind) {
    const editor = editorRef.current;
    const model = editor?.getModel();
    const at = editor?.getPosition();
    if (!editor || !model || !at) return;
    const line = model.getLineContent(at.lineNumber);
    const parsed = /^\s*\\(?:section|subsection|subsubsection|paragraph|subparagraph|chapter)\*?\{(.*)\}\s*$/.exec(line);
    const inner = parsed ? parsed[1] : line.trim();
    const next = kind === "normal" ? inner : `\\${kind}{${inner}}`;
    editor.executeEdits("moss-toolbar", [
      {
        range: { startLineNumber: at.lineNumber, startColumn: 1, endLineNumber: at.lineNumber, endColumn: line.length + 1 },
        text: next,
      },
    ]);
    editor.focus();
  }

  const askMath = (display: boolean) =>
    prompt({
      title: display ? "Display equation" : "Inline equation",
      description: "Type the LaTeX for the equation, without the dollar signs.",
      label: "LaTeX",
      initial: display ? "\\int_0^1 x^2\\,dx = \\frac{1}{3}" : "E = mc^2",
      confirmLabel: "Insert",
    });

  const sourceActions: EditActions = {
    heading: setSourceHeadingKind,
    bold: () => wrap("\\textbf{", "}", "bold text"),
    italic: () => wrap("\\textit{", "}", "italic text"),
    math: (display) => (display ? wrap("\\[\n", "\n\\]", "x") : wrap("$", "$", "x")),
    symbol: (latex) => {
      // Inside an equation the command goes in bare; elsewhere it gets its own dollar signs.
      const text = atCursor ? latex : `$${latex}$`;
      const braces = latex.endsWith("{}");
      putText(text, braces ? text.length - (atCursor ? 1 : 2) : text.length);
    },
    ref: async (label) => {
      const chosen =
        label ??
        (await prompt({
          title: "Insert reference",
          label: "Label",
          description: "The label to point to, as in \\label{...}.",
          confirmLabel: "Insert",
        }));
      if (chosen) putText(`\\ref{${chosen}}`);
    },
    cite: async () => {
      const key = await prompt({
        title: "Insert citation",
        label: "Citation key",
        description: "The key of the entry in your .bib file.",
        confirmLabel: "Insert",
      });
      if (key) putText(`\\cite{${key}}`);
    },
    table: () => putText(TABLE + "\n"),
    list: (ordered) => {
      const env = ordered ? "enumerate" : "itemize";
      const body = `\\begin{${env}}\n  \\item \n\\end{${env}}\n`;
      putText(body, body.indexOf("\\item ") + 6);
    },
  };

  const visualActions: EditActions = {
    heading: (kind) => {
      const level = kind === "section" ? 1 : kind === "subsection" ? 2 : kind === "subsubsection" ? 3 : 0;
      if (level === 0) visualEditor?.chain().focus().setParagraph().run();
      else visualEditor?.chain().focus().setHeading({ level: level as 1 | 2 | 3 }).run();
    },
    bold: () => void visualEditor?.chain().focus().toggleBold().run(),
    italic: () => void visualEditor?.chain().focus().toggleItalic().run(),
    math: async (display) => {
      const latex = await askMath(display);
      if (latex === null || !visualEditor) return;
      if (display) visualEditor.chain().focus().insertContent({ type: "blockMath", attrs: { latex } }).run();
      else
        visualEditor
          .chain()
          .focus()
          .insertContent([{ type: "inlineMath", attrs: { latex } }, { type: "text", text: " " }])
          .run();
    },
    symbol: (latex) =>
      void visualEditor
        ?.chain()
        .focus()
        .insertContent([{ type: "inlineMath", attrs: { latex: latex.replace(/\{\}$/, "{x}") } }, { type: "text", text: " " }])
        .run(),
    ref: () => {},
    cite: () => {},
    table: () =>
      void visualEditor?.chain().focus().insertContent({ type: "rawLatex", attrs: { latex: TABLE, label: "Table" } }).run(),
    list: (ordered) =>
      void (ordered
        ? visualEditor?.chain().focus().toggleOrderedList().run()
        : visualEditor?.chain().focus().toggleBulletList().run()),
  };

  if (!path) return <Placeholder>Select a file to edit</Placeholder>;
  if (!editable) {
    return <Placeholder>{path} is not a text file, so there is nothing to edit here.</Placeholder>;
  }

  const showVisual = isTex && mode === "visual";
  const language = kind === "bib" ? "bibtex" : "latex";

  return (
    <div className="flex h-full min-h-0 flex-col bg-editor">
      {isTex && (
        <EditorToolbar
          path={path}
          visual={showVisual}
          mode={mode}
          onMode={setMode}
          actions={showVisual ? visualActions : sourceActions}
          labels={labels}
          heading={
            showVisual
              ? visualEditor?.isActive("heading", { level: 1 })
                ? "section"
                : visualEditor?.isActive("heading", { level: 2 })
                  ? "subsection"
                  : visualEditor?.isActive("heading", { level: 3 })
                    ? "subsubsection"
                    : "normal"
              : sourceHeading
          }
          bold={showVisual && Boolean(visualEditor?.isActive("bold"))}
          italic={showVisual && Boolean(visualEditor?.isActive("italic"))}
          equationOpen={equationOpen}
          onToggleEquation={() => setEquationOpen((open) => !open)}
          onShowInPdf={() => {
            const at = editorRef.current?.getPosition();
            if (at) onShowInPdf(at.lineNumber, at.column);
          }}
        />
      )}

      <div className="min-h-0 flex-1">
        {showVisual ? (
          <div className="h-full overflow-auto">
            <LatexVisualEditor latex={value} onLatexChange={onVisualChange ?? onChange} onEditor={setVisualEditor} onTick={tick} />
          </div>
        ) : (
          <MonacoEditor
            height="100%"
            path={path}
            language={language}
            theme={MOSS_THEME}
            value={value}
            beforeMount={(monaco) => setupMonaco(monaco)}
            onMount={onMount}
            onChange={(next) => onChange(next ?? "")}
            options={EDITOR_OPTIONS}
          />
        )}
      </div>

      {isTex && equationOpen && !showVisual && (
        <div className="flex max-h-[45%] shrink-0 flex-col gap-3 overflow-auto border-t p-3">
          <p className="text-xs text-muted-foreground">
            {atCursor
              ? `Cursor is in a ${atCursor.displayMode ? "display" : "inline"} equation.`
              : "Put the cursor inside math to edit it, or insert a new equation."}
          </p>
          <EquationEditor value={equation} onChange={setEquation} />
          <div className="flex flex-wrap gap-2">
            <Button variant="outline" size="sm" onClick={() => insertEquation(false)}>
              Insert inline
            </Button>
            <Button variant="outline" size="sm" onClick={() => insertEquation(true)}>
              Insert display
            </Button>
            <Button variant="ghost" size="sm" onClick={loadEquation} disabled={!atCursor}>
              Load from cursor
            </Button>
            <Button size="sm" onClick={replaceEquation} disabled={!atCursor}>
              Replace at cursor
            </Button>
          </div>
        </div>
      )}
    </div>
  );
});

/**
 * One stable object: the Monaco wrapper calls `updateOptions` whenever this prop changes
 * identity, and an inline literal changed on every render (each cursor move re-renders this
 * pane). That reset the hover and Find widgets under the mouse, so they flickered and the Find
 * close button could not be clicked.
 */
const EDITOR_OPTIONS: MonacoApi.editor.IStandaloneEditorConstructionOptions = {
  minimap: { enabled: false },
  fontSize: 13,
  fontFamily: 'ui-monospace, "Cascadia Mono", Consolas, monospace',
  wordWrap: "on",
  scrollBeyondLastLine: false,
  automaticLayout: true,
  renderWhitespace: "none",
  padding: { top: 8 },
  smoothScrolling: true,
  glyphMargin: true,
  // Hovers and the Find widget are drawn on top of everything instead of being clipped by the
  // panel around the editor.
  fixedOverflowWidgets: true,
  hover: { enabled: true, delay: 250, sticky: true, above: false },
  find: { addExtraSpaceOnTop: false, seedSearchStringFromSelection: "selection" },
};
