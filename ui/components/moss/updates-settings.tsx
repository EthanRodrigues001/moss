"use client";

import { CheckCircle2, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { Updater } from "@/lib/use-updater";
import { SettingsGroup, SettingsRow } from "./settings-ui";

function when(ms: number | null): string {
  if (ms === null) return "not yet";
  return new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

/** Settings > Updates: the running version, a manual check, and installing a newer one. */
export function UpdatesSettings({ updater }: { updater: Updater }) {
  const { release, checking, installing, percent, error } = updater;
  return (
    <SettingsGroup title="Moss">
      <SettingsRow
        title="Version"
        description={
          <>
            <span className="text-foreground">{updater.version ?? "…"}</span>
            <span className="mt-1 block">
              Moss looks for a newer version a few seconds after it starts, then every hour. Last checked:{" "}
              {when(updater.checkedAt)}.
            </span>
          </>
        }
      >
        <Button variant="outline" size="sm" disabled={checking || installing} onClick={() => void updater.check()}>
          {checking && <Loader2 data-icon="inline-start" className="animate-spin" />}
          Check for updates
        </Button>
      </SettingsRow>

      {release ? (
        <SettingsRow
          title={`Moss ${release.version} is available`}
          description={
            <>
              {release.pre_release && <span className="block">This is a beta release.</span>}
              {release.notes.trim() && (
                <span className="mt-1 block max-h-40 overflow-y-auto whitespace-pre-line">{release.notes.trim()}</span>
              )}
              <span className="mt-1 block">
                Moss closes, installs the update and opens again. Your projects are not touched.
              </span>
            </>
          }
        >
          <Button size="sm" disabled={installing} onClick={() => void updater.install()}>
            {installing ? (percent === null ? "Updating…" : `Downloading ${percent}%`) : "Update and restart"}
          </Button>
        </SettingsRow>
      ) : (
        updater.checkedAt !== null &&
        !error && (
          <SettingsRow
            title={
              <span className="flex items-center gap-2">
                <CheckCircle2 className="size-4 text-emerald-400" />
                Moss is up to date
              </span>
            }
          />
        )
      )}

      {error && (
        <SettingsRow
          title="Could not check"
          description={
            <>
              <span className="block break-words text-warning">{error}</span>
              <span className="mt-1 block">
                This is normal before the first release is published, or when you are offline.
              </span>
            </>
          }
        />
      )}
    </SettingsGroup>
  );
}
