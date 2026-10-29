// The sections of a .tex file, for the file outline.

export type OutlineItem = {
  /** 0 part, 1 chapter, 2 section, 3 subsection, 4 subsubsection. */
  level: number;
  title: string;
  /** 1-based line in the file. */
  line: number;
};

const LEVELS: Record<string, number> = {
  part: 0,
  chapter: 1,
  section: 2,
  subsection: 3,
  subsubsection: 4,
};

const HEADING = /\\(part|chapter|section|subsection|subsubsection)\*?(?:\[[^\]]*\])?\{/;

export function outlineOf(source: string): OutlineItem[] {
  const items: OutlineItem[] = [];
  source.split("\n").forEach((raw, index) => {
    const line = raw.replace(/(^|[^\\])%.*/, "$1");
    const match = HEADING.exec(line);
    if (!match) return;
    // Take the braced title, allowing nested braces such as \textbf{...}.
    let depth = 1;
    let end = match.index + match[0].length;
    const start = end;
    for (; end < line.length && depth > 0; end += 1) {
      if (line[end] === "{") depth += 1;
      else if (line[end] === "}") depth -= 1;
    }
    const title = line
      .slice(start, depth === 0 ? end - 1 : end)
      .replace(/\\[a-zA-Z]+\*?\s*/g, "")
      .replace(/[{}]/g, "")
      .trim();
    if (title) items.push({ level: LEVELS[match[1]], title, line: index + 1 });
  });
  return items;
}
