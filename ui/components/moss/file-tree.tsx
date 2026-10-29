"use client";

import { useMemo, useState } from "react";
import {
  Braces,
  ChevronDown,
  ChevronRight,
  File,
  FileArchive,
  FileCode2,
  FileImage,
  FilePlus2,
  FileText,
  FolderPlus,
  Library,
  MoreVertical,
  Pencil,
  Sigma,
  Trash2,
  Upload,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ScrollArea } from "@/components/ui/scroll-area";
import { cn } from "@/lib/utils";
import { Tip } from "./tip";
import type { FileEntry, FileKind } from "@/lib/types";

type FileIcon = { Icon: typeof File; color: string };

// Colours follow the T3 Code file tree: warm orange for data, green for text-like and images,
// soft blue for LaTeX, neutral grey for anything unknown.
const BLUE = "#5ab0ff";
const PURPLE = "#c792ea";
const ORANGE = "#ffa359";
const GREEN = "#5ecc71";
const RED = "#f0625d";
const GREY = "#adadb1";

const byKind: Record<FileKind, FileIcon> = {
  tex: { Icon: Sigma, color: BLUE },
  bib: { Icon: Library, color: ORANGE },
  style: { Icon: FileCode2, color: PURPLE },
  class: { Icon: FileCode2, color: PURPLE },
  bst: { Icon: FileCode2, color: PURPLE },
  image: { Icon: FileImage, color: GREEN },
  pdf: { Icon: FileText, color: RED },
  asset: { Icon: File, color: GREY },
  folder: { Icon: File, color: GREY },
};

const byExtension: Record<string, FileIcon> = {
  json: { Icon: Braces, color: ORANGE },
  md: { Icon: FileText, color: GREEN },
  txt: { Icon: FileText, color: GREY },
  zip: { Icon: FileArchive, color: GREY },
  csv: { Icon: FileText, color: GREEN },
};

export function fileIconFor(path: string, kind: FileKind): FileIcon {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  return byExtension[ext] ?? byKind[kind];
}

export const isEditable = (kind: FileKind) =>
  kind === "tex" || kind === "bib" || kind === "style" || kind === "class" || kind === "bst";

const parentOf = (path: string) => path.split("/").slice(0, -1).join("/");

type Props = {
  files: FileEntry[];
  rootFile: string;
  active: string | null;
  onOpen: (file: FileEntry) => void;
  /** `folder` is the Project-relative folder to act in; empty means the Project root. */
  onNewFile: (folder: string) => void;
  onNewFolder: (folder: string) => void;
  onUpload: (folder: string) => void;
  onRename: (file: FileEntry) => void;
  onDelete: (file: FileEntry) => void;
};

export function FileTree({
  files,
  rootFile,
  active,
  onOpen,
  onNewFile,
  onNewFolder,
  onUpload,
  onRename,
  onDelete,
}: Props) {
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState(true);
  // The folder the header buttons act in: the last folder or file clicked.
  const [target, setTarget] = useState("");

  const visible = useMemo(
    () =>
      files.filter((f) => {
        const parts = f.path.split("/");
        for (let i = 1; i < parts.length; i++) {
          if (collapsed.has(parts.slice(0, i).join("/"))) return false;
        }
        return true;
      }),
    [files, collapsed],
  );

  function toggle(path: string) {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  const where = target || "the project root";

  return (
    <div className={cn("flex min-h-0 flex-col", open && "flex-1")}>
      <div className="flex h-9 shrink-0 items-center gap-0.5 border-b px-1.5">
        <button
          type="button"
          onClick={() => setOpen(!open)}
          className="flex min-w-0 flex-1 items-center gap-1 rounded-md px-1.5 py-1 text-sm font-semibold hover:bg-accent"
        >
          {open ? <ChevronDown className="size-3.5" /> : <ChevronRight className="size-3.5" />}
          <span className="truncate">Files</span>
          <span className="ml-1 text-xs font-normal text-muted-foreground">{files.filter((f) => !f.is_dir).length}</span>
        </button>
        <Tip label={`New file in ${where}`} side="bottom">
          <Button variant="ghost" size="icon-xs" aria-label="New file" onClick={() => onNewFile(target)}>
            <FilePlus2 />
          </Button>
        </Tip>
        <Tip label={`New folder in ${where}`} side="bottom">
          <Button variant="ghost" size="icon-xs" aria-label="New folder" onClick={() => onNewFolder(target)}>
            <FolderPlus />
          </Button>
        </Tip>
        <Tip label={`Add files from this PC to ${where}`} side="bottom">
          <Button variant="ghost" size="icon-xs" aria-label="Upload files" onClick={() => onUpload(target)}>
            <Upload />
          </Button>
        </Tip>
      </div>

      {open && (
        <ScrollArea className="min-h-0 flex-1">
          <ul className="px-1.5 py-1.5 text-sm">
            {visible.map((f) => {
              const depth = f.path.split("/").length - 1;
              const name = f.path.split("/").pop() ?? f.path;
              const { Icon, color } = fileIconFor(f.path, f.kind);
              const isOpen = !collapsed.has(f.path);
              return (
                <li key={f.path}>
                  <div
                    className={cn(
                      "group relative flex items-center rounded-md pr-1 transition-colors hover:bg-accent/70",
                      active === f.path && "bg-accent text-accent-foreground hover:bg-accent",
                    )}
                  >
                    {/* Indent guides: one faint rail per ancestor folder, joining up between rows. */}
                    {Array.from({ length: depth }, (_, i) => (
                      <span
                        key={i}
                        aria-hidden
                        style={{ left: 15 + i * 16 }}
                        className="pointer-events-none absolute inset-y-0 w-px bg-border"
                      />
                    ))}
                    <button
                      type="button"
                      style={{ paddingLeft: 7 + depth * 16 }}
                      className={cn(
                        "flex min-w-0 flex-1 items-center gap-1.5 py-1 text-left",
                        !f.is_dir && !isEditable(f.kind) && "text-muted-foreground",
                      )}
                      onClick={() => {
                        setTarget(f.is_dir ? f.path : parentOf(f.path));
                        if (f.is_dir) toggle(f.path);
                        else onOpen(f);
                      }}
                    >
                      {f.is_dir ? (
                        // Folders are just a chevron, as in T3 Code.
                        isOpen ? (
                          <ChevronDown className="size-4 shrink-0 text-[#84848a]" />
                        ) : (
                          <ChevronRight className="size-4 shrink-0 text-[#84848a]" />
                        )
                      ) : (
                        <Icon className="size-4 shrink-0" style={{ color }} />
                      )}
                      <span className={cn("truncate", f.is_dir && "text-foreground/80")} title={f.path}>
                        {name}
                      </span>
                      {f.path === rootFile && (
                        <span
                          title="Main file: this is what gets compiled"
                          className="ml-auto shrink-0 rounded px-1 text-[10px] leading-4 font-semibold tracking-wide text-primary/80 group-hover:hidden"
                        >
                          MAIN
                        </span>
                      )}
                    </button>

                    <DropdownMenu>
                      <DropdownMenuTrigger
                        render={
                          <button
                            type="button"
                            aria-label={`Actions for ${name}`}
                            className="flex size-5 shrink-0 items-center justify-center rounded-md text-muted-foreground opacity-0 group-hover:opacity-100 hover:bg-foreground/10 hover:text-foreground focus-visible:opacity-100 data-popup-open:opacity-100"
                          />
                        }
                      >
                        <MoreVertical className="size-3.5" />
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end" className="w-44">
                        {f.is_dir && (
                          <>
                            <DropdownMenuItem onClick={() => onNewFile(f.path)}>
                              <FilePlus2 /> New file
                            </DropdownMenuItem>
                            <DropdownMenuItem onClick={() => onNewFolder(f.path)}>
                              <FolderPlus /> New folder
                            </DropdownMenuItem>
                            <DropdownMenuItem onClick={() => onUpload(f.path)}>
                              <Upload /> Add files…
                            </DropdownMenuItem>
                            <DropdownMenuSeparator />
                          </>
                        )}
                        <DropdownMenuItem onClick={() => onRename(f)}>
                          <Pencil /> Rename
                        </DropdownMenuItem>
                        <DropdownMenuItem variant="destructive" onClick={() => onDelete(f)}>
                          <Trash2 /> Delete
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                </li>
              );
            })}
          </ul>
        </ScrollArea>
      )}
    </div>
  );
}
