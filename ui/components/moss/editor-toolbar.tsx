"use client";

import {
  Bold,
  BookMarked,
  ChevronDown,
  ChevronRight,
  Crosshair,
  Italic,
  Link2,
  List,
  ListOrdered,
  MoreHorizontal,
  Omega,
  Sigma,
  Table2,
  Type,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { Tip } from "./tip";

export type HeadingKind = "normal" | "section" | "subsection" | "subsubsection" | "paragraph" | "subparagraph";

/** What the toolbar can do; the editor pane implements it for the source and the visual editor. */
export type EditActions = {
  heading: (kind: HeadingKind) => void;
  bold: () => void;
  italic: () => void;
  math: (display: boolean) => void;
  symbol: (latex: string) => void;
  ref: (label: string | null) => void;
  cite: () => void;
  table: () => void;
  list: (ordered: boolean) => void;
};

const HEADINGS: { kind: HeadingKind; label: string; className: string }[] = [
  { kind: "normal", label: "Normal text", className: "text-sm" },
  { kind: "section", label: "Section", className: "text-xl font-bold" },
  { kind: "subsection", label: "Subsection", className: "text-lg font-bold" },
  { kind: "subsubsection", label: "Subsubsection", className: "text-base font-bold" },
  { kind: "paragraph", label: "Paragraph", className: "text-sm font-bold" },
  { kind: "subparagraph", label: "Subparagraph", className: "text-sm font-bold" },
];

const SYMBOLS: [string, string][] = [
  ["α", "\\alpha"], ["β", "\\beta"], ["γ", "\\gamma"], ["δ", "\\delta"], ["ε", "\\varepsilon"], ["θ", "\\theta"],
  ["λ", "\\lambda"], ["μ", "\\mu"], ["π", "\\pi"], ["σ", "\\sigma"], ["φ", "\\phi"], ["ω", "\\omega"],
  ["Δ", "\\Delta"], ["Σ", "\\Sigma"], ["Ω", "\\Omega"], ["∞", "\\infty"], ["±", "\\pm"], ["×", "\\times"],
  ["÷", "\\div"], ["≤", "\\leq"], ["≥", "\\geq"], ["≠", "\\neq"], ["≈", "\\approx"], ["→", "\\rightarrow"],
  ["←", "\\leftarrow"], ["∈", "\\in"], ["∑", "\\sum"], ["∫", "\\int"], ["√", "\\sqrt{}"], ["∂", "\\partial"],
];

type Props = {
  /** Project-relative path of the open file, shown as a breadcrumb. */
  path: string;
  visual: boolean;
  mode: "source" | "visual";
  onMode: (mode: "source" | "visual") => void;
  actions: EditActions;
  /** Labels defined in the open file, offered when inserting a reference. */
  labels: string[];
  heading: HeadingKind;
  bold: boolean;
  italic: boolean;
  equationOpen: boolean;
  onToggleEquation: () => void;
  onShowInPdf: () => void;
};

const group = "flex shrink-0 items-center gap-0.5 rounded-lg bg-muted/60 p-px";

export function EditorToolbar({
  path,
  visual,
  mode,
  onMode,
  actions,
  labels,
  heading,
  bold,
  italic,
  equationOpen,
  onToggleEquation,
  onShowInPdf,
}: Props) {
  const current = HEADINGS.find((h) => h.kind === heading) ?? HEADINGS[0];
  // Inline references and citations have no form in the visual editor, so they need the Code view.
  const codeOnly = "Switch to Code to insert this";

  return (
    <div className="flex h-9 shrink-0 items-center gap-1.5 border-b px-3">
      {/* Folders muted, file name strong; this gives way first when the pane is narrow. */}
      <div className="flex min-w-0 flex-1 items-center gap-1.5 text-sm" title={path}>
        {path
          .split("/")
          .map((part, i, all) =>
            i === all.length - 1 ? (
              <span key={i} className="truncate font-semibold">
                {part}
              </span>
            ) : (
              <span key={i} className="flex shrink-0 items-center gap-1.5 text-muted-foreground">
                {part}
                <ChevronRight className="size-3.5" />
              </span>
            ),
          )}
      </div>

      <div className={group}>
        <DropdownMenu>
          <Tip label="Text style" side="bottom">
            <DropdownMenuTrigger render={<Button variant="ghost" size="xs" aria-label="Text style" className="gap-1 px-2" />}>
              <Type />
              <ChevronDown className="size-3" />
            </DropdownMenuTrigger>
          </Tip>
          <DropdownMenuContent align="start" className="w-52">
            {HEADINGS.map((h) => (
              <DropdownMenuItem
                key={h.kind}
                disabled={visual && (h.kind === "paragraph" || h.kind === "subparagraph")}
                className={cn("py-2", h.kind === current.kind && "bg-accent")}
                onClick={() => actions.heading(h.kind)}
              >
                <span className={h.className}>{h.label}</span>
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <div className={group}>
        <Tip label="Bold" side="bottom">
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label="Bold"
            aria-pressed={bold}
            className={cn(bold && "bg-accent")}
            onClick={actions.bold}
          >
            <Bold />
          </Button>
        </Tip>
        <Tip label="Italic" side="bottom">
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label="Italic"
            aria-pressed={italic}
            className={cn(italic && "bg-accent")}
            onClick={actions.italic}
          >
            <Italic />
          </Button>
        </Tip>
      </div>

      <div className={group}>
        <DropdownMenu>
          <Tip label="Insert math" side="bottom">
            <DropdownMenuTrigger render={<Button variant="ghost" size="icon-xs" aria-label="Insert math" />}>
              <Sigma />
            </DropdownMenuTrigger>
          </Tip>
          <DropdownMenuContent align="start" className="w-52">
            <DropdownMenuItem onClick={() => actions.math(false)}>Inline math</DropdownMenuItem>
            <DropdownMenuItem onClick={() => actions.math(true)}>Display math</DropdownMenuItem>
            {!visual && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuItem onClick={onToggleEquation}>
                  {equationOpen ? "Hide equation editor" : "Equation editor…"}
                </DropdownMenuItem>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>

        <DropdownMenu>
          <Tip label="Insert symbol" side="bottom">
            <DropdownMenuTrigger render={<Button variant="ghost" size="icon-xs" aria-label="Insert symbol" />}>
              <Omega />
            </DropdownMenuTrigger>
          </Tip>
          <DropdownMenuContent align="start" className="w-auto p-2">
            <div className="grid grid-cols-6 gap-1">
              {SYMBOLS.map(([glyph, latex]) => (
                <button
                  key={latex}
                  type="button"
                  title={latex}
                  className="grid size-8 place-items-center rounded-md text-base hover:bg-accent"
                  onClick={() => actions.symbol(latex)}
                >
                  {glyph}
                </button>
              ))}
            </div>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <div className={group}>
        <DropdownMenu>
          <Tip label={visual ? codeOnly : "Insert reference"} side="bottom">
            <DropdownMenuTrigger
              render={<Button variant="ghost" size="icon-xs" aria-label="Insert reference" disabled={visual} />}
            >
              <Link2 />
            </DropdownMenuTrigger>
          </Tip>
          <DropdownMenuContent align="start" className="w-56">
            {labels.length > 0 && (
              <>
                <DropdownMenuGroup>
                  <DropdownMenuLabel>Labels in this file</DropdownMenuLabel>
                  {labels.map((l) => (
                    <DropdownMenuItem key={l} onClick={() => actions.ref(l)}>
                      <span className="truncate font-mono text-xs">{l}</span>
                    </DropdownMenuItem>
                  ))}
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
              </>
            )}
            <DropdownMenuItem onClick={() => actions.ref(null)}>Type a label…</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <Tip label={visual ? codeOnly : "Insert citation"} side="bottom">
          <Button variant="ghost" size="icon-xs" aria-label="Insert citation" disabled={visual} onClick={actions.cite}>
            <BookMarked />
          </Button>
        </Tip>
      </div>

      <DropdownMenu>
        <Tip label="More" side="bottom">
          <DropdownMenuTrigger
            render={<Button variant="ghost" size="icon-xs" aria-label="More insert options" className="shrink-0 rounded-lg bg-muted/60" />}
          >
            <MoreHorizontal />
          </DropdownMenuTrigger>
        </Tip>
        <DropdownMenuContent align="start" className="w-48">
          <DropdownMenuItem onClick={actions.table}>
            <Table2 />
            Insert table
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => actions.list(false)}>
            <List />
            Bullet list
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => actions.list(true)}>
            <ListOrdered />
            Numbered list
          </DropdownMenuItem>
          {!visual && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={onShowInPdf}>
                <Crosshair />
                Show this line in the PDF
              </DropdownMenuItem>
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>

      <div className="flex shrink-0 items-center rounded-lg bg-muted/60 p-px">
        {(["source", "visual"] as const).map((m) => (
          <button
            key={m}
            type="button"
            aria-pressed={mode === m}
            onClick={() => onMode(m)}
            className={cn(
              "rounded-md px-2 py-0.5 text-xs font-semibold transition-colors",
              mode === m ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:text-foreground",
            )}
          >
            {m === "source" ? "Code" : "Visual"}
          </button>
        ))}
      </div>
    </div>
  );
}
