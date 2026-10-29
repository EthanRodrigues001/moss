"use client";

import { Node, mergeAttributes } from "@tiptap/core";
import Placeholder from "@tiptap/extension-placeholder";
import { EditorContent, NodeViewWrapper, ReactNodeViewRenderer, useEditor, type Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import katex from "katex";
import { useEffect, useRef, useState } from "react";
import { useDialogs } from "./dialogs";
import { EquationEditor } from "./equation-editor";
import { Button } from "@/components/ui/button";
import {
  latexToTiptapDocument,
  parseVisualDocument,
  serializeVisualDocument,
  visualDocumentToTiptap,
  type VisualDocument,
} from "@/lib/latex-visual";

const InlineMath = Node.create({
  name: "inlineMath",
  group: "inline",
  inline: true,
  atom: true,
  addAttributes() {
    return { latex: { default: "" } };
  },
  parseHTML() {
    return [{ tag: "span[data-inline-math]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["span", mergeAttributes(HTMLAttributes, { "data-inline-math": "" })];
  },
  addNodeView() {
    return ReactNodeViewRenderer(MathNodeView);
  },
});

const BlockMath = Node.create({
  name: "blockMath",
  group: "block",
  atom: true,
  addAttributes() {
    return { latex: { default: "" } };
  },
  parseHTML() {
    return [{ tag: "div[data-block-math]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["div", mergeAttributes(HTMLAttributes, { "data-block-math": "" })];
  },
  addNodeView() {
    return ReactNodeViewRenderer(MathNodeView);
  },
});

const RawLatex = Node.create({
  name: "rawLatex",
  group: "block",
  atom: true,
  addAttributes() {
    return {
      latex: { default: "" },
      label: { default: "LaTeX block" },
    };
  },
  parseHTML() {
    return [{ tag: "pre[data-raw-latex]" }];
  },
  renderHTML({ HTMLAttributes }) {
    return ["pre", mergeAttributes(HTMLAttributes, { "data-raw-latex": "" })];
  },
  addNodeView() {
    return ReactNodeViewRenderer(RawLatexNodeView);
  },
});

export function LatexVisualEditor({
  latex,
  pendingSearch,
  onPendingSearchHandled,
  onLatexChange,
  onEditor,
  onTick,
}: {
  latex: string;
  pendingSearch?: string;
  onPendingSearchHandled?: () => void;
  onLatexChange: (latex: string) => void;
  /** Hands the Tiptap editor to the pane's toolbar (null when it goes away). */
  onEditor?: (editor: Editor | null) => void;
  /** The selection or content changed, so the toolbar should redraw its pressed states. */
  onTick?: () => void;
}) {
  const latestLatexRef = useRef(latex);
  const updatingFromEditorRef = useRef(false);
  /** The blocks the editor was loaded from, so untouched LaTeX is written back unchanged. */
  const parsedRef = useRef<VisualDocument | null>(null);
  /** The last LaTeX this editor wrote; coming back as the `latex` prop it needs no reload. */
  const lastEmittedRef = useRef<string | null>(null);
  const tickRef = useRef(onTick);
  useEffect(() => {
    tickRef.current = onTick;
  });

  useEffect(() => {
    latestLatexRef.current = latex;
  }, [latex]);

  const editor = useEditor({
    immediatelyRender: false,
    extensions: [
      StarterKit,
      InlineMath,
      BlockMath,
      RawLatex,
      Placeholder.configure({ placeholder: "Write visually, Moss keeps LaTeX underneath." }),
    ],
    content: latexToTiptapDocument(latex),
    editorProps: {
      attributes: {
        class:
          "min-h-full bg-background px-12 py-10 text-[15px] leading-7 outline-none prose-headings:font-semibold",
      },
    },
    onUpdate({ editor }) {
      updatingFromEditorRef.current = true;
      const original = parsedRef.current ?? parseVisualDocument(latestLatexRef.current);
      const { latex: next, parsed } = serializeVisualDocument(editor.getJSON(), original);
      parsedRef.current = parsed;
      lastEmittedRef.current = next;
      onLatexChange(next);
      tickRef.current?.();
      queueMicrotask(() => {
        updatingFromEditorRef.current = false;
      });
    },
    onSelectionUpdate() {
      tickRef.current?.();
    },
  });

  // Load changed LaTeX into the editor (an Agent edit, undo, a different file). Tiptap redraws its
  // node views with flushSync, which React forbids inside an effect, so the load is deferred to a
  // microtask. A load that would change nothing is skipped, which also keeps the caret in place.
  useEffect(() => {
    if (!editor || updatingFromEditorRef.current) return;
    let cancelled = false;
    queueMicrotask(() => {
      if (cancelled || editor.isDestroyed || updatingFromEditorRef.current) return;
      if (latex === lastEmittedRef.current) return;
      lastEmittedRef.current = null;
      const parsed = parseVisualDocument(latex);
      parsedRef.current = parsed;
      const next = visualDocumentToTiptap(parsed);
      if (JSON.stringify(next) === JSON.stringify(editor.getJSON())) return;
      editor.commands.setContent(next, { emitUpdate: false });
    });
    return () => {
      cancelled = true;
    };
  }, [editor, latex]);

  useEffect(() => {
    onEditor?.(editor);
    return () => onEditor?.(null);
  }, [editor, onEditor]);

  useEffect(() => {
    if (!editor || !pendingSearch) return;
    if (selectTextInVisualEditor(editor, pendingSearch)) {
      onPendingSearchHandled?.();
    }
  }, [editor, pendingSearch, onPendingSearchHandled]);

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      <EditorContent className="h-full min-h-0 flex-1 overflow-y-auto overflow-x-hidden [&_.tiptap]:min-h-full [&_.tiptap]:pb-24 [&_.tiptap_code]:rounded [&_.tiptap_code]:bg-muted [&_.tiptap_code]:px-1 [&_.tiptap_pre]:mb-4 [&_.tiptap_pre]:max-h-56 [&_.tiptap_pre]:overflow-auto [&_.tiptap_pre]:rounded-md [&_.tiptap_pre]:border [&_.tiptap_pre]:bg-muted/40 [&_.tiptap_pre]:p-3 [&_.tiptap_pre]:text-xs [&_.tiptap_p]:mb-3 [&_.tiptap_h1]:mb-4 [&_.tiptap_h1]:text-2xl [&_.tiptap_h2]:mb-3 [&_.tiptap_h2]:text-xl [&_.tiptap_h3]:mb-2 [&_.tiptap_h3]:text-lg [&_.tiptap_ul]:mb-4 [&_.tiptap_ul]:list-disc [&_.tiptap_ul]:pl-6 [&_.tiptap_ol]:mb-4 [&_.tiptap_ol]:list-decimal [&_.tiptap_ol]:pl-6" editor={editor} />
    </div>
  );
}

function selectTextInVisualEditor(editor: Editor, search: string) {
  const target = normalizeVisualSearch(search).toLowerCase();
  if (!target) return false;

  let found: { from: number; to: number } | null = null;
  editor.state.doc.descendants((node, position) => {
    if (found || !node.isText || !node.text) return !found;
    const normalizedText = normalizeVisualSearch(node.text).toLowerCase();
    const index = normalizedText.indexOf(target);
    if (index < 0) return true;

    found = {
      from: position + index,
      to: position + index + target.length,
    };
    return false;
  });

  if (!found) return false;
  editor.chain().focus().setTextSelection(found).run();
  requestAnimationFrame(() => {
    editor.view.dom.ownerDocument
      .getSelection()
      ?.anchorNode?.parentElement?.scrollIntoView({ block: "center", inline: "nearest" });
  });
  return true;
}

function normalizeVisualSearch(value: string) {
  return value
    .replace(/[^\w' -]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function MathNodeView({
  node,
  updateAttributes,
}: {
  node: { attrs: { latex?: string }; type: { name: string } };
  updateAttributes: (attrs: Record<string, string>) => void;
}) {
  const latex = node.attrs.latex ?? "";
  const displayMode = node.type.name === "blockMath";
  const [editing, setEditing] = useState(false);
  const [draftLatex, setDraftLatex] = useState(latex);
  const html = katex.renderToString(latex, { displayMode, throwOnError: false, strict: false });

  return (
    <NodeViewWrapper
      as={displayMode ? "div" : "span"}
      className={displayMode ? "my-4 cursor-pointer rounded-md bg-muted/50 p-3 text-center" : "mx-1 cursor-pointer rounded bg-muted px-1 py-0.5"}
      contentEditable={false}
      onClick={() => {
        setDraftLatex(latex);
        setEditing(true);
      }}
    >
      {editing ? (
        <span className={displayMode ? "flex flex-col gap-2" : "inline-flex max-w-full flex-col gap-2"} onClick={(event) => event.stopPropagation()}>
          <EquationEditor compact={!displayMode} value={draftLatex} onChange={setDraftLatex} />
          <span className="flex justify-end gap-2">
            <Button
              variant="ghost"
              size="xs"
              onClick={() => {
                setDraftLatex(latex);
                setEditing(false);
              }}
            >
              Cancel
            </Button>
            <Button
              variant="outline"
              size="xs"
              onClick={() => {
                updateAttributes({ latex: draftLatex });
                setEditing(false);
              }}
            >
              Apply
            </Button>
          </span>
        </span>
      ) : (
        <span dangerouslySetInnerHTML={{ __html: html }} />
      )}
    </NodeViewWrapper>
  );
}

function RawLatexNodeView({
  node,
  updateAttributes,
}: {
  node: { attrs: { latex?: string; label?: string } };
  updateAttributes: (attrs: Record<string, string>) => void;
}) {
  const { prompt } = useDialogs();
  const latex = node.attrs.latex ?? "";
  const label = node.attrs.label ?? "LaTeX block";

  return (
    <NodeViewWrapper className="my-3 rounded-md border bg-muted/40 p-3" contentEditable={false}>
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className="text-xs font-medium uppercase tracking-wide text-muted-foreground">{label}</span>
        <Button
          variant="outline"
          size="xs"
          onClick={async () => {
            const next = await prompt({
              title: "Edit LaTeX",
              description: `This ${label.toLowerCase()} is kept as LaTeX because it has no visual form.`,
              initial: latex,
              multiline: true,
              confirmLabel: "Save",
            });
            if (next !== null) updateAttributes({ latex: next });
          }}
        >
          Edit LaTeX
        </Button>
      </div>
      <pre className="max-h-28 overflow-auto whitespace-pre-wrap text-xs text-muted-foreground">{latex}</pre>
    </NodeViewWrapper>
  );
}
