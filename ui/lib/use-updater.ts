"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "@/lib/moss-api";
import type { UpdateRelease } from "@/lib/types";

const message = (e: unknown) => (e instanceof Error ? e.message : typeof e === "string" ? e : String(e));

export type Updater = {
  version: string | null;
  release: UpdateRelease | null;
  checking: boolean;
  checkedAt: number | null;
  /** Why the last check failed (offline, no release channel yet), or null. */
  error: string | null;
  installing: boolean;
  /** 0 to 100 while downloading, null when the size is unknown. */
  percent: number | null;
  check: () => Promise<void>;
  install: () => Promise<void>;
};

/** Looks for a newer Moss shortly after start and then hourly; Settings can also ask by hand. */
export function useUpdater(): Updater {
  const [version, setVersion] = useState<string | null>(null);
  const [release, setRelease] = useState<UpdateRelease | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkedAt, setCheckedAt] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);
  const [percent, setPercent] = useState<number | null>(null);
  const busy = useRef(false);

  const check = useCallback(async () => {
    if (busy.current) return;
    busy.current = true;
    setChecking(true);
    setError(null);
    try {
      setRelease(await api.updateCheck());
    } catch (e) {
      setError(message(e));
    } finally {
      setCheckedAt(Date.now());
      setChecking(false);
      busy.current = false;
    }
  }, []);

  const install = useCallback(async () => {
    setInstalling(true);
    setPercent(0);
    setError(null);
    const off = await api.onUpdateProgress((got, total) =>
      setPercent(total ? Math.min(100, Math.round((got / total) * 100)) : null),
    );
    try {
      await api.updateInstall(); // Moss restarts when this succeeds
    } catch (e) {
      setError(message(e));
      setInstalling(false);
    } finally {
      off();
    }
  }, []);

  useEffect(() => {
    api.appVersion().then(setVersion).catch(() => {});
    const first = window.setTimeout(() => void check(), 4000);
    const again = window.setInterval(() => void check(), 60 * 60 * 1000);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(again);
    };
  }, [check]);

  return { version, release, checking, checkedAt, error, installing, percent, check, install };
}
