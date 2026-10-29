"use client";

import { useCallback, useSyncExternalStore } from "react";

const listeners = new Set<() => void>();

function subscribe(callback: () => void) {
  listeners.add(callback);
  window.addEventListener("storage", callback);
  return () => {
    listeners.delete(callback);
    window.removeEventListener("storage", callback);
  };
}

/** A true/false setting remembered in localStorage (for things like "sidebar collapsed"). */
export function useStoredFlag(key: string, fallback: boolean): [boolean, (value: boolean) => void] {
  const value = useSyncExternalStore(
    subscribe,
    () => {
      const raw = window.localStorage.getItem(key);
      return raw === null ? fallback : raw === "1";
    },
    () => fallback,
  );
  const set = useCallback(
    (next: boolean) => {
      window.localStorage.setItem(key, next ? "1" : "0");
      listeners.forEach((l) => l());
    },
    [key],
  );
  return [value, set];
}
