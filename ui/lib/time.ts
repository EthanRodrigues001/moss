/** "just now", "5 min ago", "3 h ago", "Yesterday 14:36", "Oct 25 14:36" (unix seconds in). */
export function relativeTime(unixSeconds: number, now: number = Date.now()): string {
  const diff = Math.max(0, Math.floor(now / 1000) - unixSeconds);
  if (diff < 45) return "just now";
  if (diff < 3600) return `${Math.max(1, Math.round(diff / 60))} min ago`;
  if (diff < 3600 * 6) return `${Math.round(diff / 3600)} h ago`;
  return shortDate(unixSeconds, now);
}

const clock = (d: Date) =>
  `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;

/** "Oct 25 14:36", or "Yesterday 14:36" / "Today 14:36". */
export function shortDate(unixSeconds: number, now: number = Date.now()): string {
  const d = new Date(unixSeconds * 1000);
  const today = new Date(now);
  const startOfToday = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime();
  const day = 86_400_000;
  if (d.getTime() >= startOfToday) return `Today ${clock(d)}`;
  if (d.getTime() >= startOfToday - day) return `Yesterday ${clock(d)}`;
  const month = d.toLocaleString("en-US", { month: "short" });
  return `${month} ${d.getDate()} ${clock(d)}`;
}

export function fullDate(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function plural(n: number, word: string, many?: string): string {
  return `${n} ${n === 1 ? word : (many ?? `${word}s`)}`;
}
