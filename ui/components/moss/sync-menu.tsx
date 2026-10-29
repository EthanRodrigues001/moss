"use client";

import { ArrowDownToLine, ArrowUpFromLine, ChevronDown, Cloud, FolderOpen, Link2Off, RefreshCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ButtonGroup } from "@/components/ui/button-group";
import { Tip } from "./tip";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { plural, relativeTime } from "@/lib/time";
import type { SyncMode, SyncStatus } from "@/lib/types";

type Props = {
  status: SyncStatus | null;
  onSync: (mode: SyncMode) => void;
  onConnect: () => void;
  onUnlink: () => void;
  onOpenPracticeFolder: () => void;
};

/** Pull, Push and Sync for an Overleaf Project, or "Connect" for a Local one. */
export function SyncMenu({ status, onSync, onConnect, onUnlink, onOpenPracticeFolder }: Props) {
  if (!status) return null;

  if (!status.link) {
    return (
      <Button variant="outline" size="sm" onClick={onConnect} data-testid="connect-overleaf">
        <Cloud data-icon="inline-start" />
        Connect to Overleaf
      </Button>
    );
  }

  const ahead = status.changes.length;
  const synced = status.link.last_synced;
  return (
    <DropdownMenu>
      <ButtonGroup>
        <Tip label={synced ? `Sync both ways. Last synced ${relativeTime(synced)}` : "Sync both ways. Not synced yet"} side="bottom">
          <Button variant="outline" size="sm" data-testid="sync-now" onClick={() => onSync("sync")}>
            <RefreshCw data-icon="inline-start" />
            Sync
            {ahead > 0 && (
              <span className="rounded-full bg-primary/20 px-1.5 text-[10px] leading-4 font-semibold text-primary" aria-label={plural(ahead, "local change")}>
                {ahead}
              </span>
            )}
          </Button>
        </Tip>
        <Tip label="Pull, push and more" side="bottom">
          <DropdownMenuTrigger
            render={<Button variant="outline" size="icon-sm" data-testid="sync-menu" aria-label="More sync options" />}
          >
            <ChevronDown />
          </DropdownMenuTrigger>
        </Tip>
      </ButtonGroup>
      <DropdownMenuContent align="end" className="w-64">
        <DropdownMenuItem onClick={() => onSync("pull")}>
          <ArrowDownToLine />
          Pull from {status.practice ? "practice project" : "Overleaf"}
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => onSync("push")}>
          <ArrowUpFromLine />
          Push {ahead > 0 ? plural(ahead, "change") : "changes"}
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => onSync("sync")}>
          <RefreshCw />
          Sync both ways
        </DropdownMenuItem>
        <DropdownMenuSeparator />
        {status.practice && (
          <DropdownMenuItem onClick={onOpenPracticeFolder}>
            <FolderOpen />
            Open the practice folder
          </DropdownMenuItem>
        )}
        <DropdownMenuItem variant="destructive" onClick={onUnlink}>
          <Link2Off />
          Unlink from {status.practice ? "practice project" : "Overleaf"}…
        </DropdownMenuItem>
        <div className="px-2 py-1.5 text-xs text-muted-foreground">
          {synced ? `Last synced ${relativeTime(synced)}` : "Not synced yet"}
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
