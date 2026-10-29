"use client";

import { useEffect, useState } from "react";
import { FilePlus2, FileText, Loader2, Tag, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { api } from "@/lib/moss-api";
import { fullDate } from "@/lib/time";
import type { RemoteUpdate } from "@/lib/types";

type Props = { project: string };

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Overleaf's own change history: who edited which files, and when. Read-only. */
export function OverleafHistory({ project }: Props) {
  const [updates, setUpdates] = useState<RemoteUpdate[] | null>(null);
  const [next, setNext] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [more, setMore] = useState(false);

  useEffect(() => {
    let cancelled = false;
    api.overleafHistory(project, null).then(
      (h) => {
        if (cancelled) return;
        setUpdates(h.updates);
        setNext(h.next_before);
      },
      (e) => !cancelled && setError(message(e)),
    );
    return () => {
      cancelled = true;
    };
  }, [project]);

  async function loadMore() {
    if (next === null) return;
    setMore(true);
    try {
      const h = await api.overleafHistory(project, next);
      setUpdates((cur) => [...(cur ?? []), ...h.updates]);
      setNext(h.next_before);
    } catch (e) {
      setError(message(e));
    } finally {
      setMore(false);
    }
  }

  if (error)
    return (
      <p className="px-6 py-10 text-center text-sm text-destructive" data-testid="overleaf-history-error">
        Could not read Overleaf’s history: {error}
      </p>
    );
  if (!updates)
    return (
      <p className="flex items-center justify-center gap-2 px-6 py-10 text-sm text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        Loading Overleaf’s history…
      </p>
    );
  if (updates.length === 0)
    return <p className="px-6 py-10 text-center text-sm text-muted-foreground">Overleaf has no history for this project yet.</p>;

  return (
    <ScrollArea className="h-full" data-testid="overleaf-history">
      <ul>
        {updates.map((u) => (
          <li key={`${u.from_version}-${u.to_version}-${u.ended}`} className="border-b px-5 py-3" data-testid="overleaf-update">
            <div className="flex items-baseline gap-3">
              <span className="text-sm font-semibold">{u.ended ? fullDate(u.ended) : "Earlier"}</span>
              <span className="text-xs text-muted-foreground">{u.users.length > 0 ? u.users.join(", ") : "unknown user"}</span>
            </div>
            <ul className="mt-1.5 space-y-0.5 text-[13px]">
              {u.edited.map((p) => (
                <Line key={`e${p}`} icon={<FileText className="size-3.5 text-primary" />} verb="Edited" path={p} />
              ))}
              {u.added.map((p) => (
                <Line key={`a${p}`} icon={<FilePlus2 className="size-3.5 text-emerald-400" />} verb="Created" path={p} />
              ))}
              {u.removed.map((p) => (
                <Line key={`r${p}`} icon={<Trash2 className="size-3.5 text-destructive" />} verb="Deleted" path={p} />
              ))}
              {u.renamed.map((p) => (
                <Line key={`n${p}`} icon={<FileText className="size-3.5 text-sky-400" />} verb="Renamed" path={p} />
              ))}
              {u.labels.map((l) => (
                <li key={`l${l}`} className="flex items-center gap-1.5 text-amber-300">
                  <Tag className="size-3.5" />
                  Label: {l}
                </li>
              ))}
            </ul>
          </li>
        ))}
      </ul>
      {next !== null && (
        <div className="flex justify-center p-4">
          <Button variant="outline" size="sm" onClick={() => void loadMore()} disabled={more}>
            {more && <Loader2 data-icon="inline-start" className="animate-spin" />}
            Load older changes
          </Button>
        </div>
      )}
      <p className="px-5 pb-4 text-center text-xs text-muted-foreground">
        This is Overleaf’s own record. To get these changes here, Pull; Moss keeps its own versions of your files.
      </p>
    </ScrollArea>
  );
}

function Line({ icon, verb, path }: { icon: React.ReactNode; verb: string; path: string }) {
  return (
    <li className="flex items-center gap-1.5">
      {icon}
      <span className="text-muted-foreground">{verb}</span>
      <span className="truncate font-mono text-xs">{path}</span>
    </li>
  );
}
