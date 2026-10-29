import type { JSONContent } from "@tiptap/react";

export type LatexDocumentStyle = "article" | "ieee";

export function latexDocumentStyle(source: string): LatexDocumentStyle {
  return /\\documentclass(?:\[[^\]]*\])?\{IEEEtran\}/.test(source) || source.includes("\\IEEEauthorblockN")
    ? "ieee"
    : "article";
}

// The visual editor must never change LaTeX the user did not touch. So the body is cut into blocks
// and each block keeps the exact source it came from plus the whitespace after it. Only text that
// is plainly words (and inline maths) becomes editable text; everything else (comments, \cite,
// \ref, \textbf, \chapter, tables, equations, anything unknown) stays a raw block that is written
// back byte for byte. A block the user did not edit is written back from its source, not rebuilt.

/** One top-level piece of the body. */
export type VisualBlock = {
  node: JSONContent;
  /** The exact LaTeX this block came from (or was last written as). */
  source: string;
  /** The whitespace that followed it, so spacing survives. */
  after: string;
};

export type VisualDocument = {
  crlf: boolean;
  /** Everything up to and including `\begin{document}`; empty for a file with no document body. */
  head: string;
  /** From `\end{document}` to the end of the file. */
  tail: string;
  /** Whitespace before the first block. */
  lead: string;
  blocks: VisualBlock[];
};

const BEGIN = "\\begin{document}";
const END = "\\end{document}";

const ENVIRONMENT_LABELS: Record<string, string> = {
  abstract: "Abstract",
  IEEEkeywords: "IEEE keywords",
  figure: "Figure",
  "figure*": "Figure",
  table: "Table",
  "table*": "Table",
  tabular: "Table",
  "tabular*": "Table",
  tabularx: "Table",
  longtable: "Table",
  thebibliography: "Bibliography",
  equation: "Equation",
  "equation*": "Equation",
  align: "Equation",
  "align*": "Equation",
};

/** Lines that are always kept as they are. */
const RAW_LINE =
  /^\\(?:maketitle|FloatBarrier|label|part|chapter|section|subsection|subsubsection|paragraph|subparagraph|bibliography|bibliographystyle|end\{abstract\}|end\{IEEEkeywords\})\b/;

export function parseVisualDocument(source: string): VisualDocument {
  const crlf = source.includes("\r\n");
  const text = crlf ? source.replace(/\r\n/g, "\n") : source;
  const begin = text.indexOf(BEGIN);
  const end = text.lastIndexOf(END);
  const hasBody = begin >= 0 && end > begin;
  const head = hasBody ? text.slice(0, begin + BEGIN.length) : "";
  const tail = hasBody ? text.slice(end) : "";
  const inner = hasBody ? text.slice(begin + BEGIN.length, end) : text;
  return { crlf, head, tail, ...parseBody(inner.split("\n")) };
}

export function latexToTiptapDocument(source: string): JSONContent {
  return visualDocumentToTiptap(parseVisualDocument(source));
}

export function visualDocumentToTiptap(doc: VisualDocument): JSONContent {
  const content = doc.blocks.map((block) => block.node);
  return { type: "doc", content: content.length ? content : [{ type: "paragraph" }] };
}

/**
 * Turn the editor's document back into LaTeX. Blocks that still match one of the original blocks
 * are written from the original source; only new or edited blocks are rebuilt from the node.
 */
export function serializeVisualDocument(
  editorDoc: JSONContent,
  original: VisualDocument,
): { latex: string; parsed: VisualDocument } {
  const pool = new Map<string, { block: VisualBlock; index: number }[]>();
  original.blocks.forEach((block, index) => {
    const key = nodeKey(block.node);
    const list = pool.get(key);
    if (list) list.push({ block, index });
    else pool.set(key, [{ block, index }]);
  });

  const emitted: { block: VisualBlock; from: number }[] = [];
  for (const node of editorDoc.content ?? []) {
    const hit = pool.get(nodeKey(node))?.shift();
    if (hit) {
      emitted.push({ block: { node, source: hit.block.source, after: hit.block.after }, from: hit.index });
      continue;
    }
    const source = nodeToLatex(node);
    if (source.trim()) emitted.push({ block: { node, source, after: "\n\n" }, from: -1 });
  }

  const finalAfter = original.blocks.at(-1)?.after ?? "\n";
  emitted.forEach((item, k) => {
    const next = emitted[k + 1];
    if (!next) {
      item.block.after = finalAfter;
    } else if (!(item.from >= 0 && next.from === item.from + 1) && !item.block.after.includes("\n\n")) {
      // A new neighbour must not merge into this block's paragraph.
      item.block.after = "\n\n";
    }
  });

  const blocks = emitted.map((item) => item.block);
  const parsed: VisualDocument = { ...original, blocks };
  const latex = original.head + original.lead + blocks.map((b) => b.source + b.after).join("") + original.tail;
  return { latex: original.crlf ? latex.replace(/\n/g, "\r\n") : latex, parsed };
}

// ---------- reading LaTeX ----------

type Span = { start: number; end: number; node: JSONContent };

function parseBody(lines: string[]): { lead: string; blocks: VisualBlock[] } {
  const spans: Span[] = [];
  let paragraphStart = -1;

  const flushParagraph = (endExclusive: number) => {
    if (paragraphStart < 0) return;
    spans.push({
      start: paragraphStart,
      end: endExclusive - 1,
      node: paragraphNode(lines.slice(paragraphStart, endExclusive)),
    });
    paragraphStart = -1;
  };

  let i = 0;
  while (i < lines.length) {
    const trimmed = lines[i].trim();
    if (!trimmed) {
      flushParagraph(i);
      i += 1;
      continue;
    }

    if (paragraphStart < 0 && trimmed.startsWith("%")) {
      let end = i;
      while (end + 1 < lines.length && lines[end + 1].trim().startsWith("%")) end += 1;
      spans.push({ start: i, end, node: rawLatexNode(lines.slice(i, end + 1).join("\n"), "Comment") });
      i = end + 1;
      continue;
    }

    const structured = structuredBlock(lines, i);
    if (structured) {
      flushParagraph(i);
      spans.push({ start: i, end: structured.end, node: structured.node });
      i = structured.end + 1;
      continue;
    }

    if (paragraphStart < 0) paragraphStart = i;
    i += 1;
  }
  flushParagraph(lines.length);

  if (!spans.length) return { lead: lines.join("\n"), blocks: [] };

  const blocks = spans.map((span, k) => {
    const next = spans[k + 1];
    const after = next
      ? `\n${lines.slice(span.end + 1, next.start).map((l) => `${l}\n`).join("")}`
      : lines.length > span.end + 1
        ? `\n${lines.slice(span.end + 1).join("\n")}`
        : "";
    return { node: span.node, source: lines.slice(span.start, span.end + 1).join("\n"), after };
  });
  const lead = spans[0].start > 0 ? `${lines.slice(0, spans[0].start).join("\n")}\n` : "";
  return { lead, blocks };
}

function structuredBlock(lines: string[], i: number): { end: number; node: JSONContent } | null {
  const trimmed = lines[i].trim();

  if (trimmed.startsWith("\\[")) {
    let end = i;
    while (end < lines.length && !lines[end].includes("\\]")) end += 1;
    if (end >= lines.length) end = lines.length - 1;
    const block = lines.slice(i, end + 1);
    return { end, node: displayMathNode(block) ?? rawLatexNode(block.join("\n")) };
  }

  const begin = trimmed.match(/^\\begin\{([^}]+)\}/);
  if (begin) {
    const name = begin[1];
    const end = environmentEnd(lines, i, name);
    const block = lines.slice(i, end + 1);
    const list = name === "itemize" || name === "enumerate" ? listNode(block, name) : null;
    return { end, node: list ?? rawLatexNode(block.join("\n"), ENVIRONMENT_LABELS[name] ?? "LaTeX block") };
  }

  const heading = trimmed.match(/^\\(section|subsection|subsubsection)\{([^\\{}%$~&#_^]+)\}$/);
  if (heading) {
    return {
      end: i,
      node: {
        type: "heading",
        attrs: { level: heading[1] === "section" ? 1 : heading[1] === "subsection" ? 2 : 3 },
        content: [{ type: "text", text: heading[2] }],
      },
    };
  }

  if (RAW_LINE.test(trimmed)) return { end: i, node: rawLatexNode(lines[i]) };
  return null;
}

/** The line where `\begin{name}` on line `start` is closed (the last line if it never is). */
function environmentEnd(lines: string[], start: number, name: string) {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const open = new RegExp(`\\\\begin\\{${escaped}\\}`, "g");
  const close = new RegExp(`\\\\end\\{${escaped}\\}`, "g");
  let depth = 0;
  for (let j = start; j < lines.length; j += 1) {
    const code = lines[j].replace(/(^|[^\\])%.*/, "$1");
    depth += (code.match(open)?.length ?? 0) - (code.match(close)?.length ?? 0);
    if (depth <= 0) return j;
  }
  return lines.length - 1;
}

function displayMathNode(block: string[]): JSONContent | null {
  const inner = block.join("\n").trim().match(/^\\\[([\s\S]*)\\\]$/)?.[1];
  if (inner === undefined || inner.includes("%") || inner.includes("\\[") || inner.includes("\\]")) return null;
  return { type: "blockMath", attrs: { latex: inner.trim() } };
}

function listNode(block: string[], name: string): JSONContent | null {
  if (block.length < 2) return null;
  if (block[0].trim() !== `\\begin{${name}}` || block[block.length - 1].trim() !== `\\end{${name}}`) return null;
  const items: JSONContent[] = [];
  for (const raw of block.slice(1, -1)) {
    const line = raw.trim();
    if (!line) continue;
    const item = line.match(/^\\item\s+(.+)$/)?.[1];
    if (item === undefined || !isPlainLine(item)) return null;
    items.push({ type: "listItem", content: [{ type: "paragraph", content: inlineContent(item.trim()) }] });
  }
  return items.length ? { type: name === "itemize" ? "bulletList" : "orderedList", content: items } : null;
}

function paragraphNode(run: string[]): JSONContent {
  if (!run.every(isPlainLine)) return rawLatexNode(run.join("\n"));
  const text = run.map((line) => line.trim()).join(" ").replace(/\s+/g, " ").trim();
  return { type: "paragraph", content: inlineContent(text) };
}

/** Words and `$inline maths$` only: no commands, braces, comments or special characters. */
function isPlainLine(line: string) {
  if (line.includes("%") || line.includes("\\$") || line.includes("$$")) return false;
  return !/[\\{}$~&#_^]/.test(line.replace(/\$[^$]+\$/g, ""));
}

function inlineContent(text: string): JSONContent[] {
  return text
    .split(/(\$[^$]+\$)/g)
    .filter((part) => part.length > 0)
    .map((part) =>
      part.startsWith("$") && part.endsWith("$") && part.length > 1
        ? { type: "inlineMath", attrs: { latex: part.slice(1, -1) } }
        : { type: "text", text: part },
    );
}

function rawLatexNode(latex: string, label = "LaTeX block"): JSONContent {
  return { type: "rawLatex", attrs: { latex, label } };
}

// ---------- writing LaTeX ----------

function nodeToLatex(node: JSONContent): string {
  if (node.type === "heading") {
    const level = Number(node.attrs?.level ?? 1);
    const command = level === 1 ? "section" : level === 2 ? "subsection" : "subsubsection";
    return `\\${command}{${inlineNodesToLatex(node.content)}}`;
  }
  if (node.type === "paragraph") return inlineNodesToLatex(node.content);
  if (node.type === "blockMath") return `\\[\n${node.attrs?.latex ?? ""}\n\\]`;
  if (node.type === "rawLatex") return String(node.attrs?.latex ?? "");
  if (node.type === "codeBlock") return (node.content ?? []).map((n) => n.text ?? "").join("");
  if (node.type === "bulletList" || node.type === "orderedList") {
    const environment = node.type === "bulletList" ? "itemize" : "enumerate";
    const items = (node.content ?? []).map((item) => `\\item ${inlineNodesToLatex(item.content?.[0]?.content)}`).join("\n");
    return `\\begin{${environment}}\n${items}\n\\end{${environment}}`;
  }
  return inlineNodesToLatex(node.content);
}

function inlineNodesToLatex(nodes?: JSONContent[]): string {
  return (nodes ?? []).map((node) => {
    if (node.type === "text") return marksToLatex(escapeText(node.text ?? ""), node.marks);
    if (node.type === "inlineMath") return `$${node.attrs?.latex ?? ""}$`;
    if (node.type === "hardBreak") return "\\\\";
    return inlineNodesToLatex(node.content);
  }).join("");
}

function marksToLatex(text: string, marks?: { type: string }[]): string {
  if (!marks?.length) return text;
  return marks.reduce((value, mark) => {
    if (mark.type === "bold") return `\\textbf{${value}}`;
    if (mark.type === "italic") return `\\emph{${value}}`;
    return value;
  }, text);
}

const TEXT_ESCAPES: Record<string, string> = {
  "\\": "\\textbackslash{}",
  "{": "\\{",
  "}": "\\}",
  "&": "\\&",
  "%": "\\%",
  "#": "\\#",
  _: "\\_",
  $: "\\$",
  "^": "\\textasciicircum{}",
  "~": "\\textasciitilde{}",
};

/** Text typed in the visual editor is words; LaTeX's special characters are escaped. */
function escapeText(text: string) {
  return text.replace(/[\\{}&%#_$^~]/g, (c) => TEXT_ESCAPES[c]);
}

/** A key that ignores attributes the editor fills in by default. */
function nodeKey(node: JSONContent): string {
  return JSON.stringify(canonical(node));
}

function canonical(node: JSONContent): unknown {
  const entries = Object.entries(node.attrs ?? {}).filter(
    ([key, value]) => value !== null && value !== undefined && !(node.type === "orderedList" && key === "start" && value === 1),
  );
  return {
    type: node.type,
    attrs: entries.length ? Object.fromEntries(entries) : undefined,
    text: node.text,
    marks: node.marks,
    content: node.content?.map(canonical),
  };
}
