"use client";

import { useEffect, useRef, useState } from "react";
import { ChevronLeft, ChevronRight, X } from "lucide-react";
import { cn } from "@/lib/utils";
import type { FileEntry } from "@/lib/types";
import { Button } from "@/components/ui/button";
import { fileIconFor } from "./file-tree";

type Props = {
  tabs: FileEntry[];
  active: string | null;
  onSelect: (file: FileEntry) => void;
  onClose: (file: FileEntry) => void;
};

/** The files opened this session. The active one shows a close button; the others show on hover. */
export function EditorTabs({ tabs, active, onSelect, onClose }: Props) {
  const strip = useRef<HTMLDivElement>(null);
  const [overflow, setOverflow] = useState(false);

  // Show the arrows only when the tabs do not fit, and keep the active tab in view.
  useEffect(() => {
    const el = strip.current;
    if (!el) return;
    const measure = () => setOverflow(el.scrollWidth > el.clientWidth + 1);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    el.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest", inline: "nearest" });
    return () => observer.disconnect();
  }, [tabs, active]);

  const scrollBy = (dx: number) => strip.current?.scrollBy({ left: dx, behavior: "smooth" });

  if (tabs.length === 0) return null;
  return (
    <div className="flex h-10 shrink-0 items-center gap-1 border-b px-2">
    <div
      ref={strip}
      role="tablist"
      aria-label="Open files"
      onWheel={(e) => {
        // A vertical wheel scrolls the tabs sideways.
        if (e.deltaY !== 0 && strip.current) strip.current.scrollLeft += e.deltaY;
      }}
      className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
    >
      {tabs.map((f) => {
        const name = f.path.split("/").pop() ?? f.path;
        const { Icon, color } = fileIconFor(f.path, f.kind);
        const isActive = f.path === active;
        return (
          <div
            key={f.path}
            role="tab"
            aria-selected={isActive}
            title={f.path}
            className={cn(
              "group flex h-7 shrink-0 cursor-pointer items-center gap-1.5 rounded-lg px-2.5 text-sm transition-colors",
              isActive ? "bg-accent text-foreground" : "text-muted-foreground hover:bg-accent/50 hover:text-foreground",
            )}
            onClick={() => onSelect(f)}
            onAuxClick={(e) => {
              if (e.button === 1) onClose(f);
            }}
          >
            <Icon className={cn("size-3.5 shrink-0", !isActive && "opacity-70")} style={{ color }} />
            <span className="max-w-40 truncate">{name}</span>
            <button
              type="button"
              aria-label={`Close ${name}`}
              className={cn(
                "grid size-4 place-items-center rounded text-muted-foreground hover:bg-foreground/10 hover:text-foreground",
                !isActive && "hidden group-hover:grid",
              )}
              onClick={(e) => {
                e.stopPropagation();
                onClose(f);
              }}
            >
              <X className="size-3" />
            </button>
          </div>
        );
      })}
    </div>
    {overflow && (
      <div className="flex shrink-0 items-center gap-0.5">
        <Button variant="ghost" size="icon-xs" aria-label="Scroll tabs left" onClick={() => scrollBy(-200)}>
          <ChevronLeft />
        </Button>
        <Button variant="ghost" size="icon-xs" aria-label="Scroll tabs right" onClick={() => scrollBy(200)}>
          <ChevronRight />
        </Button>
      </div>
    )}
    </div>
  );
}
