export type LatexLintSeverity = "error" | "warning" | "info";

export type LatexLintDiagnostic = {
  message: string;
  severity: LatexLintSeverity;
  startLineNumber: number;
  startColumn: number;
  endLineNumber: number;
  endColumn: number;
};

export type LatexLogIssue = LatexLintDiagnostic & {
  title: string;
  filePath?: string;
  excerpt?: string;
  category: "error" | "warning" | "citation" | "reference" | "file" | "info";
};

type StackEntry = {
  name: string;
  line: number;
  column: number;
};

export function lintLatex(source: string): LatexLintDiagnostic[] {
  const diagnostics: LatexLintDiagnostic[] = [];
  const environmentStack: StackEntry[] = [];
  const mathStack: StackEntry[] = [];
  const lines = source.replace(/\r/g, "").split("\n");
  let firstInlineMathLine = 0;
  let firstInlineMathColumn = 0;

  lines.forEach((rawLine, index) => {
    const lineNumber = index + 1;
    const line = stripComment(rawLine);

    for (const match of line.matchAll(/\\begin([A-Za-z]+[A-Za-z0-9]*)\b/g)) {
      diagnostics.push({
        message: `This looks like a damaged environment. Did you mean \\begin{${match[1]}}?`,
        severity: "error",
        startLineNumber: lineNumber,
        startColumn: match.index + 1,
        endLineNumber: lineNumber,
        endColumn: match.index + match[0].length + 1,
      });
    }

    for (const match of line.matchAll(/\\(begin|end)\{([^}]+)\}/g)) {
      const command = match[1];
      const name = match[2];
      const entry = { name, line: lineNumber, column: match.index + 1 };
      if (command === "begin") {
        environmentStack.push(entry);
        continue;
      }

      const current = environmentStack.pop();
      if (!current) {
        diagnostics.push({
          message: `\\end{${name}} does not have a matching \\begin{${name}}.`,
          severity: "error",
          startLineNumber: lineNumber,
          startColumn: entry.column,
          endLineNumber: lineNumber,
          endColumn: entry.column + match[0].length,
        });
        continue;
      }

      if (current.name !== name) {
        diagnostics.push({
          message: `Expected \\end{${current.name}} before \\end{${name}}.`,
          severity: "error",
          startLineNumber: lineNumber,
          startColumn: entry.column,
          endLineNumber: lineNumber,
          endColumn: entry.column + match[0].length,
        });
      }
    }

    for (const match of line.matchAll(/\\\[|\\\]|\\\(|\\\)/g)) {
      const token = match[0];
      const entry = { name: token, line: lineNumber, column: match.index + 1 };
      if (token === "\\[" || token === "\\(") {
        mathStack.push(entry);
        continue;
      }
      const expectedOpen = token === "\\]" ? "\\[" : "\\(";
      const current = mathStack.pop();
      if (!current || current.name !== expectedOpen) {
        diagnostics.push({
          message: `${token} does not have a matching ${expectedOpen}.`,
          severity: "error",
          startLineNumber: lineNumber,
          startColumn: entry.column,
          endLineNumber: lineNumber,
          endColumn: entry.column + token.length,
        });
      }
    }

    for (const column of unescapedDollarColumns(line)) {
      if (firstInlineMathLine) {
        firstInlineMathLine = 0;
        firstInlineMathColumn = 0;
      } else {
        firstInlineMathLine = lineNumber;
        firstInlineMathColumn = column;
      }
    }
  });

  for (const entry of environmentStack) {
    diagnostics.push({
      message: `Missing \\end{${entry.name}}.`,
      severity: "error",
      startLineNumber: entry.line,
      startColumn: entry.column,
      endLineNumber: entry.line,
      endColumn: entry.column + entry.name.length + 8,
    });
  }

  for (const entry of mathStack) {
    diagnostics.push({
      message: `Missing closing ${entry.name === "\\[" ? "\\]" : "\\)"}.`,
      severity: "error",
      startLineNumber: entry.line,
      startColumn: entry.column,
      endLineNumber: entry.line,
      endColumn: entry.column + entry.name.length,
    });
  }

  if (firstInlineMathLine) {
    diagnostics.push({
      message: "Inline math opened with $ but was not closed.",
      severity: "warning",
      startLineNumber: firstInlineMathLine,
      startColumn: firstInlineMathColumn,
      endLineNumber: firstInlineMathLine,
      endColumn: firstInlineMathColumn + 1,
    });
  }

  return diagnostics;
}

export function latexLogDiagnostic(log: string): LatexLintDiagnostic | null {
  return parseLatexLogIssues(log).find((issue) => issue.severity === "error") ?? null;
}

export function parseLatexLogIssues(log: string): LatexLogIssue[] {
  const issues: LatexLogIssue[] = [];
  const seen = new Set<string>();

  const push = (issue: LatexLogIssue) => {
    const key = `${issue.title}:${issue.startLineNumber}:${issue.message}`;
    if (seen.has(key)) return;
    seen.add(key);
    issues.push(issue);
  };

  for (const match of log.matchAll(/LaTeX Warning: Citation [`']([^`']+)[`'].*?undefined on input line (\d+)/g)) {
    const line = Number(match[2]);
    push(logIssue({
      title: `Citation '${match[1]}' is undefined`,
      message: "Add this key to a .bib file or fix the cite key.",
      category: "citation",
      severity: "warning",
      line,
    }));
  }

  for (const match of log.matchAll(/LaTeX Warning: Reference [`']([^`']+)[`'].*?undefined on input line (\d+)/g)) {
    const line = Number(match[2]);
    push(logIssue({
      title: `Reference '${match[1]}' is undefined`,
      message: "Add the matching \\label{...} or compile again after labels are generated.",
      category: "reference",
      severity: "warning",
      line,
    }));
  }

  for (const match of log.matchAll(/File [`']([^`']+)[`'] not found on input line (\d+)/g)) {
    const line = Number(match[2]);
    push(logIssue({
      title: `File '${match[1]}' not found`,
      message: "Upload the file into the project tree or correct the relative path.",
      category: "file",
      severity: "error",
      line,
    }));
  }

  for (const match of log.matchAll(/LaTeX Error: File [`']([^`']+)[`'] not found\./g)) {
    const nearbyLine = lineNear(log, match.index ?? 0);
    push(logIssue({
      title: `File '${match[1]}' not found`,
      message: "Upload the file into the project tree or correct the relative path.",
      category: "file",
      severity: "error",
      line: nearbyLine,
    }));
  }

  for (const match of log.matchAll(/(?:Package ([^ \n]+) Error|LaTeX Error):\s*([^\n]+)/g)) {
    const line = lineNear(log, match.index ?? 0);
    push(logIssue({
      title: match[1] ? `Package ${match[1]} error` : "LaTeX error",
      message: match[2].trim(),
      category: "error",
      severity: "error",
      line,
      excerpt: excerptNear(log, match.index ?? 0),
    }));
  }

  if (/warning: errors were issued by BibTeX[^\n]*/i.test(log)) {
    push(logIssue({
      title: "BibTeX reported citation issues",
      message: "Check missing citation keys, bibliography files, or BibTeX syntax.",
      category: "citation",
      severity: "warning",
      line: 1,
    }));
  }

  for (const match of log.matchAll(/LaTeX Font Warning:\s*([^\n]+)/g)) {
    const line = lineNear(log, match.index ?? 0);
    push(logIssue({
      title: "Font warning",
      message: match[1].trim(),
      category: "info",
      severity: "info",
      line,
    }));
  }

  return issues;
}

function logIssue({
  category,
  excerpt,
  line,
  message,
  severity,
  title,
}: {
  category: LatexLogIssue["category"];
  excerpt?: string;
  line: number;
  message: string;
  severity: LatexLintSeverity;
  title: string;
}): LatexLogIssue {
  return {
    title,
    message,
    severity,
    category,
    excerpt,
    startLineNumber: Math.max(1, line),
    startColumn: 1,
    endLineNumber: Math.max(1, line),
    endColumn: 120,
  };
}

function lineNear(log: string, index: number) {
  const after = log.slice(index, index + 900);
  const before = log.slice(Math.max(0, index - 900), index);
  const lineMatch = after.match(/l\.(\d+)/) ?? before.match(/input line (\d+)/) ?? after.match(/input line (\d+)/);
  if (!lineMatch) return 1;
  const lineNumber = Number(lineMatch[1]);
  return Number.isFinite(lineNumber) && lineNumber > 0 ? lineNumber : 1;
}

function excerptNear(log: string, index: number) {
  const lineMatch = log.slice(index, index + 700).match(/l\.\d+\s+([\s\S]*?)(?:\n\n|\n[A-Z][A-Za-z]+|\n\(|$)/);
  return lineMatch?.[1]?.trim();
}

function stripComment(line: string) {
  let escaped = false;
  for (let index = 0; index < line.length; index += 1) {
    const char = line[index];
    if (char === "\\" && !escaped) {
      escaped = true;
      continue;
    }
    if (char === "%" && !escaped) return line.slice(0, index);
    escaped = false;
  }
  return line;
}

function unescapedDollarColumns(line: string) {
  const columns: number[] = [];
  let escaped = false;
  for (let index = 0; index < line.length; index += 1) {
    const char = line[index];
    if (char === "\\" && !escaped) {
      escaped = true;
      continue;
    }
    if (char === "$" && !escaped) columns.push(index + 1);
    escaped = false;
  }
  return columns;
}
