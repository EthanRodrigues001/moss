"use client";

import { useState } from "react";
import { ArrowLeft, Cloud, FileArchive, FilePlus2, FlaskConical, FolderInput, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

type Source = {
  id: "new" | "zip" | "folder" | "overleaf" | "practice";
  title: string;
  subtitle: string;
  icon: typeof Search;
};

const SOURCES: Source[] = [
  { id: "new", title: "New project", subtitle: "Start an empty LaTeX project from a name", icon: FilePlus2 },
  { id: "zip", title: "Import zip", subtitle: "Create a project from a .zip file on this PC", icon: FileArchive },
  { id: "folder", title: "Import folder", subtitle: "Create a project from a folder on this PC", icon: FolderInput },
  { id: "overleaf", title: "Import from Overleaf", subtitle: "Pull a project from your Overleaf account", icon: Cloud },
  { id: "practice", title: "Practice project", subtitle: "Try Pull, Push and conflicts with a folder standing in for Overleaf", icon: FlaskConical },
];

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreate: (name: string) => void;
  onImportZip: () => void;
  onImportFolder: () => void;
  onOverleaf: () => void;
  onPractice: () => void;
};

/** One place to add a project, in the style of the T3 Code "add project" palette. */
export function AddProjectDialog(props: Props) {
  return (
    <Dialog open={props.open} onOpenChange={props.onOpenChange}>
      <DialogContent
        showCloseButton={false}
        className="top-[18%] translate-y-0 gap-0 overflow-hidden p-0 sm:max-w-xl"
      >
        <DialogTitle className="sr-only">Add a project</DialogTitle>
        <DialogDescription className="sr-only">
          Create a new project or bring one in from your PC or Overleaf.
        </DialogDescription>
        {/* Mounted only while open, so it always starts on the list. */}
        {props.open && <Palette {...props} />}
      </DialogContent>
    </Dialog>
  );
}

function Palette({ onOpenChange, onCreate, onImportZip, onImportFolder, onOverleaf, onPractice }: Props) {
  const [step, setStep] = useState<"sources" | "name">("sources");
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const [name, setName] = useState("");

  const shown = SOURCES.filter((s) =>
    `${s.title} ${s.subtitle}`.toLowerCase().includes(query.trim().toLowerCase()),
  );
  const active = Math.min(index, Math.max(0, shown.length - 1));

  function choose(source: Source) {
    if (source.id === "new") {
      setStep("name");
      return;
    }
    onOpenChange(false);
    if (source.id === "zip") onImportZip();
    if (source.id === "folder") onImportFolder();
    if (source.id === "overleaf") onOverleaf();
    if (source.id === "practice") onPractice();
  }

  function create() {
    const trimmed = name.trim();
    if (!trimmed) return;
    onOpenChange(false);
    onCreate(trimmed);
  }

  if (step === "name") {
    return (
      <form
        onSubmit={(e) => {
          e.preventDefault();
          create();
        }}
      >
        <div className="flex items-center gap-2 border-b px-4 py-3">
          <Button type="button" variant="ghost" size="icon-xs" aria-label="Back" onClick={() => setStep("sources")}>
            <ArrowLeft />
          </Button>
          <Input
            autoFocus
            value={name}
            placeholder="Project name"
            onChange={(e) => setName(e.target.value)}
            className="h-8 border-0 bg-transparent px-1 text-base shadow-none focus-visible:ring-0"
          />
        </div>
        <div className="flex items-center justify-between gap-3 px-4 py-3">
          <span className="text-xs text-muted-foreground">
            Creates a folder with a starter main.tex. You can rename things later.
          </span>
          <Button type="submit" size="sm" disabled={!name.trim()}>
            Create
          </Button>
        </div>
      </form>
    );
  }

  return (
    <div
      onKeyDown={(e) => {
        if (e.key === "ArrowDown") {
          e.preventDefault();
          setIndex(Math.min(shown.length - 1, active + 1));
        } else if (e.key === "ArrowUp") {
          e.preventDefault();
          setIndex(Math.max(0, active - 1));
        } else if (e.key === "Enter" && shown[active]) {
          e.preventDefault();
          choose(shown[active]);
        }
      }}
    >
      <div className="flex items-center gap-3 border-b px-4 py-3">
        <Search className="size-4 text-muted-foreground" />
        <Input
          autoFocus
          value={query}
          placeholder="Add a project…"
          onChange={(e) => {
            setQuery(e.target.value);
            setIndex(0);
          }}
          className="h-8 border-0 bg-transparent px-0 text-base shadow-none focus-visible:ring-0"
        />
      </div>

      <div className="px-2 pt-3 pb-2">
        <div className="px-3 pb-1.5 text-xs font-medium text-muted-foreground">Sources</div>
        <ul>
          {shown.length === 0 && (
            <li className="px-3 py-6 text-center text-sm text-muted-foreground">Nothing matches.</li>
          )}
          {shown.map((s, i) => (
            <li key={s.id}>
              <button
                type="button"
                onMouseMove={() => setIndex(i)}
                onClick={() => choose(s)}
                className={cn(
                  "flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left",
                  i === active && "bg-accent",
                )}
              >
                <s.icon className="size-4 shrink-0 text-muted-foreground" />
                <span className="min-w-0">
                  <span className="block text-sm font-medium">{s.title}</span>
                  <span className="block truncate text-xs text-muted-foreground">{s.subtitle}</span>
                </span>
              </button>
            </li>
          ))}
        </ul>
      </div>

      <div className="flex items-center gap-4 border-t px-4 py-2.5 text-xs text-muted-foreground">
        <Hint keys="↑ ↓" label="Navigate" />
        <Hint keys="Enter" label="Select" />
        <Hint keys="Esc" label="Close" />
      </div>
    </div>
  );
}

function Hint({ keys, label }: { keys: string; label: string }) {
  return (
    <span className="flex items-center gap-1.5">
      <kbd className="rounded-md bg-foreground/[0.08] px-1.5 py-0.5 font-sans text-[11px]">{keys}</kbd>
      {label}
    </span>
  );
}
