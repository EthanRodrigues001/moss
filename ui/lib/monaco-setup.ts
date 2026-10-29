// Monaco is served from `public/vendor/monaco` (copied by scripts/copy-vendor.mjs), never a CDN.

import { loader } from "@monaco-editor/react";
import type * as Monaco from "monaco-editor";

export const MOSS_THEME = "moss-dark";

let configured = false;

/** Point the loader at the bundled copy. Safe to call more than once. */
export function configureMonacoLoader() {
  loader.config({ paths: { vs: "/vendor/monaco/vs" } });
}

/** Register the LaTeX and BibTeX languages and the T3 colours once per page. */
export function setupMonaco(monaco: typeof Monaco) {
  if (configured) return;
  configured = true;

  monaco.languages.register({ id: "latex" });
  monaco.languages.setLanguageConfiguration("latex", {
    comments: { lineComment: "%" },
    brackets: [
      ["{", "}"],
      ["[", "]"],
      ["(", ")"],
    ],
    autoClosingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: "$", close: "$", notIn: ["comment"] },
    ],
    surroundingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "$", close: "$" },
    ],
  });
  monaco.languages.setMonarchTokensProvider("latex", {
    tokenizer: {
      root: [
        [/%.*$/, "comment"],
        [/\\(begin|end)(?=\s*\{)/, "keyword.control"],
        [/\\(documentclass|usepackage|input|include|includegraphics|bibliography|bibliographystyle|newcommand|renewcommand|def)\b/, "keyword"],
        [/\\(label|ref|eqref|cite[a-z]*|autoref|pageref)\b/, "type"],
        [/\\(section|subsection|subsubsection|chapter|part|paragraph|caption|title|author|date)\*?/, "string.escape"],
        [/\\[a-zA-Z@]+\*?/, "tag"],
        [/\\./, "tag"],
        [/\$\$/, { token: "number", next: "@displayMath" }],
        [/\$/, { token: "number", next: "@inlineMath" }],
        [/[{}[\]]/, "delimiter.bracket"],
        [/&|\\\\/, "delimiter"],
      ],
      inlineMath: [
        [/\$/, { token: "number", next: "@pop" }],
        [/\\[a-zA-Z]+/, "tag"],
        [/[^$\\]+/, "number"],
        [/./, "number"],
      ],
      displayMath: [
        [/\$\$/, { token: "number", next: "@pop" }],
        [/\\[a-zA-Z]+/, "tag"],
        [/[^$\\]+/, "number"],
        [/./, "number"],
      ],
    },
  });

  monaco.languages.register({ id: "bibtex" });
  monaco.languages.setLanguageConfiguration("bibtex", {
    comments: { lineComment: "%" },
    brackets: [["{", "}"]],
    autoClosingPairs: [
      { open: "{", close: "}" },
      { open: '"', close: '"' },
    ],
  });
  monaco.languages.setMonarchTokensProvider("bibtex", {
    tokenizer: {
      root: [
        [/%.*$/, "comment"],
        [/@[a-zA-Z]+/, "keyword"],
        [/[a-zA-Z_][\w-]*(?=\s*=)/, "type"],
        [/"[^"]*"/, "string"],
        [/[{}]/, "delimiter.bracket"],
      ],
    },
  });

  // T3 Code dark palette (see ui/app/globals.css).
  monaco.editor.defineTheme(MOSS_THEME, {
    base: "vs-dark",
    inherit: true,
    rules: [
      { token: "comment", foreground: "6b7280", fontStyle: "italic" },
      { token: "keyword", foreground: "7aa2ff" },
      { token: "keyword.control", foreground: "c792ea" },
      { token: "tag", foreground: "7aa2ff" },
      { token: "type", foreground: "4fd1c5" },
      { token: "string", foreground: "e6b450" },
      { token: "string.escape", foreground: "ffb900", fontStyle: "bold" },
      { token: "number", foreground: "ff9e64" },
      { token: "delimiter", foreground: "818181" },
      { token: "delimiter.bracket", foreground: "9ca3af" },
    ],
    colors: {
      "editor.background": "#111111",
      "editor.foreground": "#f5f5f5",
      "editorLineNumber.foreground": "#4a4a4a",
      "editorLineNumber.activeForeground": "#a3a3a3",
      "editor.lineHighlightBackground": "#171717",
      "editor.selectionBackground": "#346bf155",
      "editor.inactiveSelectionBackground": "#346bf130",
      "editorCursor.foreground": "#346bf1",
      "editorIndentGuide.background1": "#1f1f1f",
      "editorGutter.background": "#111111",
      "editorWidget.background": "#171717",
      "editorWidget.border": "#222222",
      "editorSuggestWidget.background": "#171717",
      "editorSuggestWidget.border": "#222222",
      "editorSuggestWidget.selectedBackground": "#1f1f1f",
      "scrollbarSlider.background": "#ffffff14",
      "scrollbarSlider.hoverBackground": "#ffffff24",
      "editorError.foreground": "#ff6467",
      "editorWarning.foreground": "#ffb900",
    },
  });
}
