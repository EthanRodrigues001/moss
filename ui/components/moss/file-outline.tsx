"use client";

import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { ScrollArea } from "@/components/ui/scroll-area";
import { outlineOf } from "@/lib/outline";
import { cn } from "@/lib/utils";

type Props = {
  /** The text of the open file, or null when no .tex file is open. */
  source: string | null;
  onJump: (line: number) => void;
};

/** The sections of the open file as a timeline: a spine with a dot per heading; click to jump. */
export function FileOutline({ source, onJump }: Props) {
  const [open, setOpen] = useState(true);
  const items = useMemo(() => (source === null ? [] : outlineOf(source)), [source]);
  // Indent relative to the shallowest heading present, so a file with only sections starts flush.
  const base = items.length > 0 ? Math.min(...items.map((i) => i.level)) : 0;
  // 1, 1.1, 1.2, 2 ...: the numbers LaTeX will give each heading.
  const numbers = useMemo(() => {
    const counts: number[] = [];
    return items.map((item) => {
      const d = item.level - base;
      counts.length = d + 1;
      for (let i = 0; i <= d; i++) counts[i] = (counts[i] ?? (i < d ? 1 : 0)) + (i === d ? 1 : 0);
      return counts.slice(0, d + 1).join(".");
    });
  }, [items, base]);

  return (
    <div className={cn("flex min-h-0 flex-col border-t", open && "max-h-[45%] min-h-24 flex-1")}>
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="flex h-9 shrink-0 items-center gap-1 px-3 text-left text-sm font-semibold hover:bg-accent/60"
      >
        {open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
        File outline
        {items.length > 0 && <span className="ml-1 text-xs font-normal text-muted-foreground">{items.length}</span>}
      </button>
      {open && (
        <ScrollArea className="min-h-0 flex-1">
          {items.length === 0 ? (
            <p className="px-4 py-3 text-xs text-muted-foreground">
              {source === null
                ? "Open a .tex file to see its sections."
                : "No sections or subsections in this file."}
            </p>
          ) : (
            <ul className="px-2 pb-2 text-sm">
              {items.map((item, i) => {
                const depth = item.level - base;
                const top = depth === 0;
                return (
                  <li key={`${item.line}-${item.title}`} className="relative">
                    {/* The spine joins each dot to the next; it starts and ends at the first and last dot. */}
                    <span
                      aria-hidden
                      className={cn(
                        "pointer-events-none absolute left-[11px] w-px bg-border",
                        i === 0 ? "top-1/2" : "top-0",
                        i === items.length - 1 ? "bottom-1/2" : "bottom-0",
                      )}
                    />
                    <button
                      type="button"
                      onClick={() => onJump(item.line)}
                      title={`Line ${item.line}`}
                      style={{ paddingLeft: 26 + depth * 10 }}
                      className={cn(
                        "group relative block w-full truncate rounded-md py-1 pr-3 text-left transition-colors hover:bg-accent/70",
                        !top && "text-muted-foreground hover:text-foreground",
                      )}
                    >
                      <span
                        aria-hidden
                        className={cn(
                          "absolute top-1/2 -translate-x-1/2 -translate-y-1/2 rounded-full transition-colors",
                          top
                            ? "left-[11px] size-2 bg-primary ring-4 ring-background group-hover:bg-foreground"
                            : "left-[11px] size-1.5 bg-muted-foreground/60 ring-4 ring-background group-hover:bg-primary",
                        )}
                      />
                      <span className="flex items-baseline gap-2">
                        <span className="w-7 shrink-0 font-mono text-[10px] text-muted-foreground/70 tabular-nums">
                          {numbers[i]}
                        </span>
                        <span className={cn("truncate", top && "font-medium")}>{item.title}</span>
                        <span className="ml-auto shrink-0 font-mono text-[10px] text-muted-foreground/0 transition-colors group-hover:text-muted-foreground">
                          L{item.line}
                        </span>
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </ScrollArea>
      )}
    </div>
  );
}
