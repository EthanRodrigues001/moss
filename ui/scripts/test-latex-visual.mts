// Run: node scripts/test-latex-visual.mts   (from ui/, Node 22.6+)
import assert from "node:assert/strict";
import test from "node:test";
import {
  latexToTiptapDocument,
  parseVisualDocument,
  serializeVisualDocument,
} from "../lib/latex-visual.ts";

const FULL = String.raw`\documentclass{report}
\usepackage{graphicx}
% a preamble comment
\begin{document}

\chapter{Introduction}\label{ch:intro}
% TODO: tighten this paragraph
Plain words with $x^2$ maths in them
over two lines.

As shown in \cite{a,b} and Section~\ref{sec:x}, we use \textbf{bold} text.
\section{Method}
Next paragraph straight after a heading.
\section*{Unnumbered}

\begin{equation}
  E = mc^2 \label{eq:e}
\end{equation}

\[
  a + b
\]

\begin{table}[h]
\begin{tabular}{ll}
a & b \\
\end{tabular}
\end{table}

\begin{itemize}
  \item first
  \item second \cite{z}
\end{itemize}

\begin{itemize}
  \item one
  \item two
\end{itemize}

\bibliographystyle{plain}
\bibliography{refs}
\end{document}
`;

/** What the editor hands back when nothing was edited. */
function untouched(source: string) {
  const original = parseVisualDocument(source);
  const doc = JSON.parse(JSON.stringify(latexToTiptapDocument(source)));
  return { original, doc, out: serializeVisualDocument(doc, original) };
}

test("an untouched document is written back byte for byte", () => {
  assert.equal(untouched(FULL).out.latex, FULL);
});

test("CRLF line endings survive", () => {
  const crlf = FULL.replace(/\n/g, "\r\n");
  assert.equal(untouched(crlf).out.latex, crlf);
});

test("a chapter file with no \\begin{document} survives", () => {
  const chapter = String.raw`\chapter{One} % keep
Words here.

More words with \cite{k}.
`;
  assert.equal(untouched(chapter).out.latex, chapter);
});

test("a file with no trailing newline and odd blank lines survives", () => {
  const odd = "\\begin{document}\n\n\n\nAlpha beta.\n\n\n\\section{S}\nGamma.\n\\end{document}";
  assert.equal(untouched(odd).out.latex, odd);
});

test("comments, \\cite, \\chapter and equations are kept as raw blocks", () => {
  const types = latexToTiptapDocument(FULL).content!.map((n) => n.type);
  assert.deepEqual(types.slice(0, 3), ["rawLatex", "rawLatex", "paragraph"]);
  const raws = latexToTiptapDocument(FULL).content!.filter((n) => n.type === "rawLatex");
  const text = raws.map((n) => String(n.attrs?.latex)).join("\n");
  for (const kept of ["% TODO", "\\chapter{Introduction}", "\\cite{a,b}", "\\begin{equation}", "\\begin{table}", "\\section*{Unnumbered}"]) {
    assert.ok(text.includes(kept), `${kept} should stay raw`);
  }
});

test("editing one paragraph changes only that paragraph", () => {
  const { original, doc } = untouched(FULL);
  const edited = JSON.parse(JSON.stringify(doc));
  const index = edited.content.findIndex((n: { type: string }) => n.type === "paragraph");
  edited.content[index].content = [{ type: "text", text: "Rewritten words." }];
  const { latex } = serializeVisualDocument(edited, original);
  // The only side effect: the comment above it now has a blank line before the edited paragraph,
  // so new text can never merge into a neighbour.
  assert.equal(latex, FULL.replace("Plain words with $x^2$ maths in them\nover two lines.", "\nRewritten words."));
});

test("a new paragraph does not merge into its neighbour", () => {
  const source = "\\begin{document}\nFirst words.\n\\end{document}\n";
  const { original, doc } = untouched(source);
  const edited = JSON.parse(JSON.stringify(doc));
  edited.content.push({ type: "paragraph", content: [{ type: "text", text: "Second words." }] });
  const { latex } = serializeVisualDocument(edited, original);
  assert.equal(latex, "\\begin{document}\nFirst words.\n\nSecond words.\n\\end{document}\n");
});

test("deleting a block leaves the rest alone", () => {
  const { original, doc } = untouched(FULL);
  const edited = JSON.parse(JSON.stringify(doc));
  edited.content = edited.content.filter((n: { type: string; attrs?: { latex?: string } }) => !String(n.attrs?.latex ?? "").startsWith("% TODO"));
  const { latex } = serializeVisualDocument(edited, original);
  // The chapter line is followed by a block it was not next to before, so a blank line separates them.
  assert.equal(latex, FULL.replace("% TODO: tighten this paragraph\n", "\n"));
});

test("special characters typed in a paragraph are escaped", () => {
  const source = "\\begin{document}\nHello.\n\\end{document}\n";
  const { original, doc } = untouched(source);
  const edited = JSON.parse(JSON.stringify(doc));
  edited.content[0].content = [{ type: "text", text: "100% of R&D_x #1 {a} ~ ^" }];
  const { latex } = serializeVisualDocument(edited, original);
  assert.ok(latex.includes("100\\% of R\\&D\\_x \\#1 \\{a\\} \\textasciitilde{} \\textasciicircum{}"), latex);
});

test("the parse of what we wrote is the same as what we hold", () => {
  const { original, doc } = untouched(FULL);
  const edited = JSON.parse(JSON.stringify(doc));
  const index = edited.content.findIndex((n: { type: string }) => n.type === "paragraph");
  edited.content[index].content = [{ type: "text", text: "Changed." }];
  const { latex, parsed } = serializeVisualDocument(edited, original);
  assert.equal(serializeVisualDocument(JSON.parse(JSON.stringify(latexToTiptapDocument(latex))), parsed).latex, latex);
});
