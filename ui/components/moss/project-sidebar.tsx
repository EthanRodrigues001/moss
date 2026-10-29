"use client";

import { useState } from "react";
import {
  AlertTriangle,
  Archive,
  ArchiveRestore,
  Bot,
  CheckCircle2,
  ChevronDown,
  Link2Off,
  ListFilter,
  MoreHorizontal,
  Pencil,
  PanelLeftClose,
  Plus,
  Settings,
  Trash2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { Tip } from "./tip";
import { MossMark } from "./moss-mark";
import { UpdateButton } from "./update-button";
import { Contours } from "@/components/dithercn/contours";
import { cn } from "@/lib/utils";
import type { ProjectSummary, TexStatus } from "@/lib/types";
import type { Updater } from "@/lib/use-updater";
import { Cloud, HardDrive } from "lucide-react";
import type { Origin } from "@/lib/types";

/** Icon-only Local / Overleaf marker: green cloud when linked, muted drive when not. */
function OriginIcon({ origin }: { origin: Origin }) {
  const overleaf = origin === "overleaf";
  const Icon = overleaf ? Cloud : HardDrive;
  return (
    <Icon
      aria-label={overleaf ? "Overleaf" : "Local"}
      className={cn("size-4", overleaf ? "text-emerald-400" : "text-muted-foreground")}
    />
  );
}

type Props = {
  projects: ProjectSummary[];
  selected: string | null;
  tex: TexStatus | null;
  updater: Updater;
  busy: boolean;
  collapsed: boolean;
  onCollapsedChange: (collapsed: boolean) => void;
  onSelect: (dirName: string) => void;
  onAddProject: () => void;
  onRename: (project: ProjectSummary) => void;
  onArchive: (project: ProjectSummary, archived: boolean) => void;
  onDelete: (project: ProjectSummary) => void;
  onUnlink: (project: ProjectSummary) => void;
  onOpenAgents: () => void;
  onOpenSettings: () => void;
};

// Paper is the sidebar's own colour and ink is the theme's primary, so the map blends in.
const CONTOURS_THEME = { "--dither-paper": "var(--sidebar)" } as React.CSSProperties;

type SortBy = "recent" | "name";
type Show = "all" | "local" | "overleaf";

/** "3h ago" style age; empty when the Project has no files yet. */
function ago(ms: number | null): string {
  if (ms === null) return "";
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 60) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.round(h / 24);
  if (d < 30) return `${d}d ago`;
  return new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export function ProjectSidebar({
  projects,
  selected,
  tex,
  updater,
  busy,
  collapsed,
  onCollapsedChange,
  onSelect,
  onAddProject,
  onRename,
  onArchive,
  onDelete,
  onUnlink,
  onOpenAgents,
  onOpenSettings,
}: Props) {
  // Always starts collapsed; the shelf is out of the way until asked for.
  const [archiveOpen, setArchiveOpen] = useState(false);
  const [sortBy, setSortBy] = useState<SortBy>("recent");
  const [show, setShow] = useState<Show>("all");
  const [moreBelow, setMoreBelow] = useState(false);

  // Closed means gone: the open button lives in the header of the main area.
  if (collapsed) return null;

  const order = (a: ProjectSummary, b: ProjectSummary) =>
    sortBy === "name"
      ? a.name.localeCompare(b.name, undefined, { sensitivity: "base" })
      : (b.last_edited_ms ?? 0) - (a.last_edited_ms ?? 0);
  const active = projects.filter((p) => !p.archived && (show === "all" || p.origin === show)).sort(order);
  const archived = projects.filter((p) => p.archived).sort(order);
  const hasActive = projects.some((p) => !p.archived);
  const filtered = sortBy !== "recent" || show !== "all";

  const row = (p: ProjectSummary) => (
    <ProjectRow
      key={p.dir_name}
      project={p}
      selected={selected === p.dir_name}
      onSelect={onSelect}
      onRename={onRename}
      onArchive={onArchive}
      onDelete={onDelete}
      onUnlink={onUnlink}
    />
  );

  const checkScroll = (el: HTMLElement) => setMoreBelow(el.scrollHeight - el.scrollTop - el.clientHeight > 4);

  return (
    <aside className="relative flex h-full w-72 shrink-0 flex-col border-r bg-sidebar text-sidebar-foreground">
      {/* A contour map behind the header only, fading out before the project list. The mask also
          fades it out under the logo, so the mark stays clean. */}
      <div
        aria-hidden
        style={{
          ...CONTOURS_THEME,
          maskImage: "linear-gradient(to bottom, black 0%, transparent 100%), linear-gradient(to right, transparent 0%, black 45%)",
          maskComposite: "intersect",
          WebkitMaskImage: "linear-gradient(to bottom, black 0%, transparent 100%), linear-gradient(to right, transparent 0%, black 45%)",
          WebkitMaskComposite: "source-in",
        }}
        className="pointer-events-none absolute inset-x-0 top-0 h-36 opacity-60"
      >
        <Contours relief={0} scale={2.2} lines={20} cell={2} className="absolute inset-0" />
      </div>

      <div className="relative flex items-center justify-between px-4 pt-4 pb-3">
        <h1 className="flex items-center gap-2.5">
          <MossMark className="size-8" />
          <span className="bg-gradient-to-b from-white to-white/65 bg-clip-text text-xl leading-none font-bold tracking-tight text-transparent">
            Moss
          </span>
        </h1>
        <Tip label="Hide projects" side="bottom">
          <Button
            variant="outline"
            size="icon-xs"
            aria-label="Hide projects"
            className="border-sidebar-border bg-sidebar text-sidebar-foreground hover:bg-sidebar-accent"
            onClick={() => onCollapsedChange(true)}
          >
            <PanelLeftClose />
          </Button>
        </Tip>
      </div>

      <div className="relative px-3 pb-3">
        <Button className="w-full" disabled={busy} onClick={onAddProject}>
          <Plus data-icon="inline-start" />
          Add project
        </Button>
      </div>

      <div className="relative flex items-center justify-between pr-3 pb-1 pl-4">
        <span className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Projects</span>
        <DropdownMenu>
          <Tip label="Filter and sort" side="bottom">
            <DropdownMenuTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon-xs"
                  aria-label="Filter and sort projects"
                  className={cn("text-muted-foreground", filtered && "text-primary")}
                />
              }
            >
              <ListFilter />
            </DropdownMenuTrigger>
          </Tip>
          <DropdownMenuContent align="end" className="w-48">
            <DropdownMenuGroup>
              <DropdownMenuLabel>Sort by</DropdownMenuLabel>
              <DropdownMenuRadioGroup value={sortBy} onValueChange={(v) => setSortBy(v as SortBy)}>
                <DropdownMenuRadioItem value="recent">Recently edited</DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="name">Name</DropdownMenuRadioItem>
              </DropdownMenuRadioGroup>
            </DropdownMenuGroup>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              <DropdownMenuLabel>Show</DropdownMenuLabel>
              <DropdownMenuRadioGroup value={show} onValueChange={(v) => setShow(v as Show)}>
                <DropdownMenuRadioItem value="all">All projects</DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="local">Local only</DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="overleaf">Overleaf only</DropdownMenuRadioItem>
              </DropdownMenuRadioGroup>
            </DropdownMenuGroup>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
      <ScrollArea className="relative min-h-0 flex-1">
        <ul className="flex flex-col gap-0.5 px-2 pb-2">
          {active.length === 0 && (
            <li className="px-2 py-6 text-center text-sm text-muted-foreground">
              {!hasActive && archived.length > 0
                ? "All your projects are archived."
                : hasActive
                  ? "No projects match this filter."
                  : "No projects yet. Add one to get started."}
            </li>
          )}
          {active.map(row)}
        </ul>
      </ScrollArea>

      {archived.length > 0 && (
        <div className="relative px-4 pt-1">
          <button
            type="button"
            aria-expanded={archiveOpen}
            onClick={() => setArchiveOpen(!archiveOpen)}
            className="flex w-full items-center gap-3 py-1.5 text-sm font-medium text-muted-foreground"
          >
            <span className="shrink-0">Archive ({archived.length})</span>
            <span className="h-px flex-1 bg-sidebar-foreground/15" />
            <ChevronDown className={cn("size-4 shrink-0 transition-transform", archiveOpen && "rotate-180")} />
          </button>
          {archiveOpen && (
            <div className="relative -mx-2">
              <ul
                onScroll={(e) => checkScroll(e.currentTarget)}
                ref={(el) => {
                  if (el) checkScroll(el);
                }}
                className="flex max-h-44 flex-col gap-0.5 overflow-y-auto pb-1 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
              >
                {archived.map(row)}
              </ul>
              {moreBelow && (
                <div className="pointer-events-none absolute inset-x-0 bottom-0 flex h-8 items-end justify-center bg-gradient-to-t from-sidebar to-transparent">
                  <ChevronDown className="size-4 animate-bounce text-muted-foreground" />
                </div>
              )}
            </div>
          )}
        </div>
      )}

      <div className="relative flex items-center justify-between gap-2 px-3 py-2 text-xs">
        <div className="flex items-center gap-1">
          <Tip label="Settings">
            <Button variant="ghost" size="icon-sm" aria-label="Settings" onClick={onOpenSettings}>
              <Settings />
            </Button>
          </Tip>
          <Tip label="Agents">
            <Button variant="ghost" size="icon-sm" aria-label="Agents" onClick={onOpenAgents}>
              <Bot />
            </Button>
          </Tip>
        </div>
        <div className="flex min-w-0 items-center gap-2">
        {tex === null ? (
          <span className="text-muted-foreground">Checking TeX…</span>
        ) : tex.found && tex.install ? (
          <span className="flex items-center gap-1.5 text-muted-foreground">
            <CheckCircle2 className="size-3.5 text-emerald-400" />
            {tex.install.flavor} ready
          </span>
        ) : (
          <span className="flex items-start gap-1.5 text-warning">
            <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
            {tex.problem ?? "TeX not found"}
          </span>
        )}
        <UpdateButton updater={updater} />
        </div>
      </div>
    </aside>
  );
}

function ProjectRow({
  project: p,
  selected,
  onSelect,
  onRename,
  onArchive,
  onDelete,
  onUnlink,
}: {
  project: ProjectSummary;
  selected: boolean;
  onSelect: (dirName: string) => void;
  onRename: (project: ProjectSummary) => void;
  onArchive: (project: ProjectSummary, archived: boolean) => void;
  onDelete: (project: ProjectSummary) => void;
  onUnlink: (project: ProjectSummary) => void;
}) {
  const actions = "opacity-0 group-focus-within/row:opacity-100 group-hover/row:opacity-100 aria-expanded:opacity-100";
  return (
    <li className="group/row relative">
      <Tooltip>
        <TooltipTrigger
          delay={600}
          render={
            <button
              type="button"
              onClick={() => onSelect(p.dir_name)}
              className={cn(
                "flex w-full items-center gap-2.5 rounded-lg px-3 py-2 text-left transition-colors hover:bg-sidebar-hover",
                selected && "bg-sidebar-accent hover:bg-sidebar-accent",
                p.archived && "opacity-60 hover:opacity-100",
              )}
            />
          }
        >
          <OriginIcon origin={p.origin} />
          {/* Hovering reserves room on the right for the row's buttons, so the name never runs under them. */}
          <span className="flex min-w-0 flex-1 items-center justify-between gap-2 group-focus-within/row:pr-[5.25rem] group-hover/row:pr-[5.25rem]">
            <span
              className="truncate text-[0.9rem] font-medium"
              onDoubleClick={(e) => {
                e.stopPropagation();
                onRename(p);
              }}
            >
              {p.name}
            </span>
            <span className="shrink-0 text-xs text-muted-foreground group-focus-within/row:hidden group-hover/row:hidden">
              {ago(p.last_edited_ms)}
            </span>
          </span>
        </TooltipTrigger>
        <TooltipContent
          side="right"
          sideOffset={10}
          className="flex-col items-start gap-2 border bg-popover p-3 text-popover-foreground shadow-lg [&>div:last-child]:hidden"
        >
          <span className="text-sm font-semibold">{p.name}</span>
          <span className="flex items-center gap-2">
            <OriginIcon origin={p.origin} />
            {p.origin === "overleaf" ? "Linked to Overleaf" : "Local only"}
          </span>
          {p.overleaf && p.overleaf.name !== p.name && (
            <span className="text-muted-foreground">Overleaf name: {p.overleaf.name}</span>
          )}
          <span className="text-muted-foreground">
            Last edited: {p.last_edited_ms ? new Date(p.last_edited_ms).toLocaleString() : "never"}
          </span>
          <span className="text-muted-foreground">Double-click the name to rename</span>
        </TooltipContent>
      </Tooltip>

      <div className="absolute top-1/2 right-2 flex -translate-y-1/2 items-center">
        <Tip label={p.archived ? "Move back to Projects" : "Archive"}>
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={p.archived ? `Move ${p.name} back to Projects` : `Archive ${p.name}`}
            className={actions}
            onClick={() => onArchive(p, !p.archived)}
          >
            {p.archived ? <ArchiveRestore /> : <Archive />}
          </Button>
        </Tip>
        <Tip label="Delete project…">
          <Button
            variant="ghost"
            size="icon-xs"
            aria-label={`Delete ${p.name}`}
            className={cn(actions, "hover:text-destructive")}
            onClick={() => onDelete(p)}
          >
            <Trash2 />
          </Button>
        </Tip>
        <DropdownMenu>
          <Tip label="More">
            <DropdownMenuTrigger
              render={<Button variant="ghost" size="icon-xs" aria-label={`More actions for ${p.name}`} className={actions} />}
            >
              <MoreHorizontal />
            </DropdownMenuTrigger>
          </Tip>
          <DropdownMenuContent align="end" className="w-56">
            <DropdownMenuItem onClick={() => onRename(p)}>
              <Pencil />
              Rename…
            </DropdownMenuItem>
            {p.origin === "overleaf" && (
              <DropdownMenuItem onClick={() => onUnlink(p)}>
                <Link2Off />
                Disconnect from Overleaf…
              </DropdownMenuItem>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </li>
  );
}
