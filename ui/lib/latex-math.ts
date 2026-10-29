// Finding the equation under the cursor so the equation editor can load and replace it.

export type EquationSelection = {
  open: string;
  close: string;
  displayMode: boolean;
  /** Offsets of the whole equation including its delimiters. */
  startOffset: number;
  endOffset: number;
  latex: string;
};

type Delimiter = { open: string; close: string; displayMode: boolean };

const DELIMITERS: Delimiter[] = [
  { open: "\\[", close: "\\]", displayMode: true },
  { open: "\\(", close: "\\)", displayMode: false },
  { open: "$$", close: "$$", displayMode: true },
  { open: "$", close: "$", displayMode: false },
];

export function findEquationAtOffset(source: string, offset: number): EquationSelection | null {
  for (const delimiter of DELIMITERS) {
    const found = findDelimited(source, offset, delimiter);
    if (found) return found;
  }
  return null;
}

function findDelimited(source: string, offset: number, d: Delimiter): EquationSelection | null {
  let from = 0;
  while (from < source.length) {
    const start = findDelimiter(source, d.open, from);
    if (start < 0) return null;
    const contentStart = start + d.open.length;
    const close = findDelimiter(source, d.close, contentStart);
    if (close < 0) return null;
    const end = close + d.close.length;
    if (offset >= contentStart && offset <= close) {
      return {
        open: d.open,
        close: d.close,
        displayMode: d.displayMode,
        startOffset: start,
        endOffset: end,
        latex: source.slice(contentStart, close).trim(),
      };
    }
    from = end;
  }
  return null;
}

function findDelimiter(source: string, delimiter: string, from: number): number {
  let index = source.indexOf(delimiter, from);
  while (index >= 0) {
    if (isUsable(source, delimiter, index)) return index;
    index = source.indexOf(delimiter, index + 1);
  }
  return -1;
}

function isUsable(source: string, delimiter: string, index: number): boolean {
  let slashes = 0;
  for (let i = index - 1; i >= 0 && source[i] === "\\"; i -= 1) slashes += 1;
  if (slashes % 2 === 1) return false; // `\$` is a literal dollar sign
  if (delimiter === "$") return source[index - 1] !== "$" && source[index + 1] !== "$";
  return true;
}
