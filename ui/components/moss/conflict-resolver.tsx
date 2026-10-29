"use client";

import { useState } from "react";
import { Check, ChevronDown, ChevronRight, Pencil } from "lucide-react";
import { Button } from "@/components/ui/button";
import { composeRegions, conflictCount, type Draft, emptyDraft, hasConflictMarkers } from "@/lib/merge";
import { plural } from "@/lib/time";
import type { Choice, Conflict, ConflictKind, Region } from "@/lib/types";
import { cn } from "@/lib/utils";

type Props = {
  conflict: Conflict;
  draft: Draft | undefined;
  onChange: (draft: Draft | undefined) => void;
};

const KIND_TEXT: Record<ConflictKind, string> = {
  both_modified: "Changed on this PC and on Overleaf, in the same place.",
  both_added: "A file with this name was added on this PC and on Overleaf, with different content.",
  modified_deleted: "Changed on this PC, but deleted on Overleaf.",
  deleted_modified: "Deleted on this PC, but changed on Overleaf.",
};

/** One file Moss could not merge on its own: pick Mine, Theirs, both, or write the text. */
export function ConflictResolver({ conflict, draft, onChange }: Props) {
  const [editing, setEditing] = useState(false);
  const regions = conflict.regions;
  const choices = draft?.kind === "regions" ? draft.choices : [];

  const setRegion = (index: number, choice: Choice | null) => {
    const base = draft?.kind === "regions" ? draft.choices : (emptyDraft(conflict) as { choices: (Choice | null)[] }).choices;
    const next = [...base];
    next[index] = choice;
    onChange({ kind: "regions", choices: next });
  };

  const currentText = () =>
    regions
      ? composeRegions(regions, choices)
      : (draft?.kind === "text" ? draft.text : (conflict.mine ?? conflict.theirs ?? ""));

  const whole = draft?.kind === "whole" ? draft.side : null;
  const canEdit = !conflict.binary && (conflict.mine !== null || conflict.theirs !== null);

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="conflict-resolver">
      <div className="space-y-2 border-b px-4 py-3">
        <p className="font-mono text-[13px] font-semibold break-all">{conflict.path}</p>
        <p className="text-xs text-muted-foreground">{KIND_TEXT[conflict.kind]}</p>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            size="sm"
            variant={whole === "mine" ? "default" : "outline"}
            onClick={() => {
              setEditing(false);
              onChange({ kind: "whole", side: "mine" });
            }}
          >
            {whole === "mine" && <Check data-icon="inline-start" />}
            {conflict.mine_exists ? "Keep mine (this PC)" : "Keep it deleted"}
          </Button>
          <Button
            size="sm"
            variant={whole === "theirs" ? "default" : "outline"}
            onClick={() => {
              setEditing(false);
              onChange({ kind: "whole", side: "theirs" });
            }}
          >
            {whole === "theirs" && <Check data-icon="inline-start" />}
            {conflict.theirs_exists ? "Keep theirs (Overleaf)" : "Accept the deletion"}
          </Button>
          {canEdit && (
            <Button
              size="sm"
              variant={draft?.kind === "text" ? "default" : "outline"}
              onClick={() => {
                const text = currentText();
                setEditing(true);
                onChange({ kind: "text", text });
              }}
            >
              <Pencil data-icon="inline-start" />
              Edit the result
            </Button>
          )}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto">
        {conflict.binary ? (
          <Side2 conflict={conflict} note="Binary files cannot be merged; choose which copy to keep." />
        ) : draft?.kind === "text" || editing ? (
          <HandEdit
            text={draft?.kind === "text" ? draft.text : currentText()}
            onChange={(text) => onChange({ kind: "text", text })}
          />
        ) : regions ? (
          <RegionList regions={regions} choices={choices} onPick={setRegion} dimmed={whole !== null} />
        ) : (
          <Side2 conflict={conflict} />
        )}
      </div>
    </div>
  );
}

function CodeBlock({ text, tone }: { text: string; tone: "mine" | "theirs" | "base" | "plain" }) {
  return (
    <pre
      className={cn(
        "min-h-8 overflow-x-auto px-3 py-2 font-mono text-xs leading-5 whitespace-pre-wrap break-words",
        tone === "mine" && "bg-sky-500/10",
        tone === "theirs" && "bg-emerald-500/10",
        tone === "base" && "bg-muted/50 text-muted-foreground",
      )}
    >
      {text === "" ? <span className="text-muted-foreground italic">(nothing)</span> : text}
    </pre>
  );
}

function Stable({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  const lines = text.replace(/\n$/, "").split("\n");
  if (lines.length <= 6 || open) {
    return (
      <pre className="px-3 py-1 font-mono text-xs leading-5 whitespace-pre-wrap break-words text-muted-foreground">
        {text}
      </pre>
    );
  }
  return (
    <div className="text-muted-foreground">
      <pre className="px-3 pt-1 font-mono text-xs leading-5 whitespace-pre-wrap">{lines.slice(0, 2).join("\n")}</pre>
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="my-0.5 flex w-full items-center gap-1 bg-muted/40 px-3 py-0.5 text-left text-xs hover:bg-muted"
      >
        <ChevronRight className="size-3.5" />
        {plural(lines.length - 4, "unchanged line")}
      </button>
      <pre className="px-3 pb-1 font-mono text-xs leading-5 whitespace-pre-wrap">{lines.slice(-2).join("\n")}</pre>
    </div>
  );
}

function RegionList({
  regions,
  choices,
  onPick,
  dimmed,
}: {
  regions: Region[];
  choices: (Choice | null)[];
  onPick: (index: number, choice: Choice | null) => void;
  dimmed: boolean;
}) {
  const total = conflictCount(regions);
  // The position of each conflicting region among the conflicts (-1 for settled text).
  const order: number[] = [];
  let seen = 0;
  for (const region of regions) order.push(region.type === "conflict" ? seen++ : -1);
  return (
    <div className={cn("py-1", dimmed && "opacity-40")}>
      {regions.map((region, i) => {
        if (region.type === "stable") return <Stable key={i} text={region.text} />;
        const n = order[i];
        return (
          <ConflictRegion
            key={i}
            region={region}
            label={`Conflict ${n + 1} of ${total}`}
            choice={choices[n] ?? null}
            onPick={(c) => onPick(n, c)}
          />
        );
      })}
    </div>
  );
}

function ConflictRegion({
  region,
  label,
  choice,
  onPick,
}: {
  region: Extract<Region, { type: "conflict" }>;
  label: string;
  choice: Choice | null;
  onPick: (choice: Choice | null) => void;
}) {
  const [showBase, setShowBase] = useState(false);
  const [editText, setEditText] = useState<string | null>(null);
  const pick = (type: "mine" | "theirs" | "both") => {
    setEditText(null);
    onPick({ type });
  };
  const btn = (type: "mine" | "theirs" | "both", text: string) => (
    <Button size="xs" variant={choice?.type === type ? "default" : "outline"} onClick={() => pick(type)}>
      {choice?.type === type && <Check data-icon="inline-start" />}
      {text}
    </Button>
  );

  return (
    <div
      className={cn("my-2 border-y", choice ? "border-emerald-500/40" : "border-amber-500/50")}
      data-testid="conflict-region"
      data-resolved={choice !== null}
    >
      <div className="flex flex-wrap items-center gap-2 bg-muted/30 px-3 py-1.5">
        <span className={cn("text-xs font-semibold", choice ? "text-emerald-400" : "text-amber-400")}>
          {label}
          {choice ? " · answered" : " · needs an answer"}
        </span>
        <div className="ml-auto flex items-center gap-1.5">
          {btn("mine", "Mine")}
          {btn("theirs", "Theirs")}
          {btn("both", "Both")}
          <Button
            size="xs"
            variant={choice?.type === "text" ? "default" : "outline"}
            onClick={() => {
              const start =
                choice?.type === "text"
                  ? choice.text
                  : choice?.type === "theirs"
                    ? region.theirs
                    : region.mine;
              setEditText(start);
              onPick({ type: "text", text: start });
            }}
          >
            <Pencil data-icon="inline-start" />
            Edit
          </Button>
          <Button size="xs" variant="ghost" onClick={() => setShowBase(!showBase)} aria-expanded={showBase}>
            {showBase ? <ChevronDown data-icon="inline-start" /> : <ChevronRight data-icon="inline-start" />}
            Original
          </Button>
        </div>
      </div>
      {editText !== null || choice?.type === "text" ? (
        <textarea
          aria-label="Edit this part by hand"
          value={choice?.type === "text" ? choice.text : (editText ?? "")}
          onChange={(e) => {
            setEditText(e.target.value);
            onPick({ type: "text", text: e.target.value });
          }}
          rows={Math.min(14, Math.max(3, (choice?.type === "text" ? choice.text : "").split("\n").length + 1))}
          className="block w-full resize-y bg-background px-3 py-2 font-mono text-xs leading-5 outline-none"
        />
      ) : (
        <div className="grid grid-cols-2 divide-x">
          <div>
            <p className="px-3 pt-1.5 text-[10px] font-semibold tracking-wider text-sky-300 uppercase">Mine (this PC)</p>
            <CodeBlock text={region.mine} tone="mine" />
          </div>
          <div>
            <p className="px-3 pt-1.5 text-[10px] font-semibold tracking-wider text-emerald-300 uppercase">Theirs (Overleaf)</p>
            <CodeBlock text={region.theirs} tone="theirs" />
          </div>
        </div>
      )}
      {showBase && (
        <div className="border-t">
          <p className="px-3 pt-1.5 text-[10px] font-semibold tracking-wider text-muted-foreground uppercase">
            Original (before either side changed it)
          </p>
          <CodeBlock text={region.base} tone="base" />
        </div>
      )}
    </div>
  );
}

function HandEdit({ text, onChange }: { text: string; onChange: (text: string) => void }) {
  return (
    <div className="flex h-full min-h-0 flex-col">
      <textarea
        aria-label="Final text of this file"
        value={text}
        onChange={(e) => onChange(e.target.value)}
        spellCheck={false}
        className="min-h-48 flex-1 resize-none bg-background px-4 py-3 font-mono text-xs leading-5 outline-none"
      />
      {hasConflictMarkers(text) && (
        <p className="border-t bg-amber-500/10 px-4 py-2 text-xs text-amber-300">
          This text still has conflict markers (&lt;&lt;&lt;&lt;&lt;&lt;&lt; or &gt;&gt;&gt;&gt;&gt;&gt;&gt;). Remove them before applying.
        </p>
      )}
    </div>
  );
}

/** Both whole files side by side, for conflicts that are not a line-by-line merge. */
function Side2({ conflict, note }: { conflict: Conflict; note?: string }) {
  const show = (exists: boolean, text: string | null, tone: "mine" | "theirs") => {
    if (!exists) return <p className="px-3 py-3 text-xs text-muted-foreground italic">Deleted on this side.</p>;
    if (text === null) return <p className="px-3 py-3 text-xs text-muted-foreground italic">Not shown (binary or large file).</p>;
    return <CodeBlock text={text} tone={tone} />;
  };
  return (
    <div>
      {note && <p className="border-b px-4 py-2 text-xs text-muted-foreground">{note}</p>}
      <div className="grid grid-cols-2 divide-x">
        <div>
          <p className="px-3 pt-2 text-[10px] font-semibold tracking-wider text-sky-300 uppercase">Mine (this PC)</p>
          {show(conflict.mine_exists, conflict.mine, "mine")}
        </div>
        <div>
          <p className="px-3 pt-2 text-[10px] font-semibold tracking-wider text-emerald-300 uppercase">Theirs (Overleaf)</p>
          {show(conflict.theirs_exists, conflict.theirs, "theirs")}
        </div>
      </div>
    </div>
  );
}
