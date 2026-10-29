// Which source line to ask SyncTeX about for a caret position.
//
// Blank lines, comments and page breaks produce no text of their own, and SyncTeX answers them
// with whatever box is nearest, often the page number at the bottom of the page. Anchor to the
// text next to them instead.

export type Anchor = {
  line: number;
  column: number;
  /** How far through the source line the caret is: 0 at the start, 1 at the end. */
  fraction: number;
};

const PAGE_BREAK = /^\s*\\(newpage|clearpage|cleardoublepage|pagebreak)\b/;

/** True when the line makes no text of its own. */
export function producesNoText(line: string): boolean {
  const trimmed = line.trim();
  return trimmed === "" || trimmed.startsWith("%") || PAGE_BREAK.test(line);
}

/**
 * `lines` are the file's lines, `line` and `column` the caret (both 1-based). A line with no text
 * of its own anchors to the end of the text above it, or the start of the text below it when
 * there is nothing above.
 */
export function syncAnchor(lines: string[], line: number, column: number): Anchor {
  const own = lines[line - 1] ?? "";
  if (!producesNoText(own)) {
    return { line, column: 1, fraction: Math.min(1, Math.max(0, (column - 1) / Math.max(1, own.length))) };
  }
  for (let i = line - 1; i >= 1; i -= 1) {
    if (!producesNoText(lines[i - 1])) return { line: i, column: 1, fraction: 1 };
  }
  for (let i = line + 1; i <= lines.length; i += 1) {
    if (!producesNoText(lines[i - 1])) return { line: i, column: 1, fraction: 0 };
  }
  return { line, column, fraction: 0 };
}
