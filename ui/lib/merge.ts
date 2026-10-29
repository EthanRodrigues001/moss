// Builds the final text of a conflicted file from the user's per-region answers. Mirrors
// `Merged::resolve` in crates/moss-core/src/textdiff.rs, so what the Conflict screen previews is
// exactly what the Rust side writes.

import type { Choice, Conflict, FileChoice, Region } from "./types";

export const conflictCount = (regions: Region[]) =>
  regions.filter((r) => r.type === "conflict").length;

/** Mine, then Theirs, with a newline between when Mine does not end with one. */
function both(mine: string, theirs: string) {
  return mine + (mine !== "" && !mine.endsWith("\n") ? "\n" : "") + theirs;
}

/** The text for one answer to one conflicting region. */
export function pickRegion(region: Extract<Region, { type: "conflict" }>, choice: Choice): string {
  switch (choice.type) {
    case "mine":
      return region.mine;
    case "theirs":
      return region.theirs;
    case "both":
      return both(region.mine, region.theirs);
    case "text":
      return choice.text;
  }
}

/**
 * The file with every answered conflict resolved. An unanswered one keeps Mine, so the preview
 * is always complete text; `choices` has one slot per conflicting region, in order.
 */
export function composeRegions(regions: Region[], choices: (Choice | null)[]): string {
  let next = 0;
  let out = "";
  for (const region of regions) {
    if (region.type === "stable") {
      out += region.text;
      continue;
    }
    out += pickRegion(region, choices[next++] ?? { type: "mine" });
  }
  return out;
}

/** True while some conflicting region still has no answer. */
export function hasUnanswered(regions: Region[], choices: (Choice | null)[]): boolean {
  return choices.length < conflictCount(regions) || choices.some((c) => c === null);
}

/** Lines starting with these are what a half-finished merge leaves behind. */
export function hasConflictMarkers(text: string): boolean {
  return /^(<<<<<<<|>>>>>>>)/m.test(text);
}

// ----- Conflict screen state -----

/** What the user has done so far for one conflicting file. */
export type Draft =
  | { kind: "whole"; side: "mine" | "theirs" }
  | { kind: "regions"; choices: (Choice | null)[] }
  | { kind: "text"; text: string };

/** The answer to send, or null while the file is not fully decided. */
export function choiceFrom(draft: Draft | undefined): FileChoice | null {
  if (!draft) return null;
  switch (draft.kind) {
    case "whole":
      return { type: draft.side };
    case "text":
      return { type: "text", text: draft.text };
    case "regions":
      return draft.choices.some((c) => c === null)
        ? null
        : { type: "regions", choices: draft.choices as Choice[] };
  }
}

/** A fresh draft with every region unanswered (text conflicts) or nothing (whole-file ones). */
export function emptyDraft(conflict: Conflict): Draft | undefined {
  return conflict.regions
    ? { kind: "regions", choices: new Array(conflictCount(conflict.regions)).fill(null) }
    : undefined;
}

/** How many of the conflict's decisions are still open. */
export function openCount(conflict: Conflict, draft: Draft | undefined): number {
  if (!draft) return conflict.regions ? conflictCount(conflict.regions) : 1;
  if (draft.kind === "regions") return draft.choices.filter((c) => c === null).length;
  return 0;
}
