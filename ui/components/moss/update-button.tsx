"use client";

import { ArrowUp, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import type { Updater } from "@/lib/use-updater";
import { Tip } from "./tip";

/** The round arrow in the sidebar footer: appears only when a newer Moss has been published. */
export function UpdateButton({ updater }: { updater: Updater }) {
  const { release } = updater;
  if (!release) return null;
  const notes = release.notes.trim();
  return (
    <DropdownMenu>
      <Tip label={`Moss ${release.version} is available`}>
        <DropdownMenuTrigger
          render={
            <Button
              variant="outline"
              size="icon"
              aria-label={`Update to Moss ${release.version}`}
              className="rounded-full border-primary/40 bg-primary/10 text-primary hover:bg-primary/20"
            />
          }
        >
          {updater.installing ? <Loader2 className="animate-spin" /> : <ArrowUp />}
        </DropdownMenuTrigger>
      </Tip>
      <DropdownMenuContent align="end" side="top" className="w-72 p-3">
        <div className="flex flex-col gap-2">
          <p className="text-sm font-semibold">Moss {release.version} is out</p>
          <p className="text-xs text-muted-foreground">
            You have {release.current}
            {release.pre_release ? ", and this is a beta" : ""}.
          </p>
          {notes && (
            <p className="max-h-32 overflow-y-auto text-xs whitespace-pre-line text-muted-foreground">
              {notes.length > 500 ? `${notes.slice(0, 500)}…` : notes}
            </p>
          )}
          <Button size="sm" disabled={updater.installing} onClick={() => void updater.install()}>
            {updater.installing
              ? updater.percent === null
                ? "Updating…"
                : `Downloading ${updater.percent}%`
              : "Update and restart"}
          </Button>
          {updater.error && <p className="text-xs text-warning">{updater.error}</p>}
        </div>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
