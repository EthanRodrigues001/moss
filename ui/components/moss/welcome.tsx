import { AlertTriangle, Bot, CheckCircle2, Cloud, FolderInput, Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { TexStatus } from "@/lib/types";
import { Aurora } from "@/components/dithercn/aurora";
import { MossMark } from "./moss-mark";

// The sky is the app background, so the aurora blends into the window.
const SKY = { "--dither-paper": "var(--background)" } as React.CSSProperties;

type Props = {
  tex: TexStatus | null;
  busy: boolean;
  onAdd: () => void;
  onSetup: () => void;
};

/** Shown when no Project is open: what Moss is, one clear first step, and whether LaTeX is ready. */
export function Welcome({ tex, busy, onAdd, onSetup }: Props) {
  return (
    <div className="relative isolate flex flex-1 flex-col items-center justify-center gap-8 overflow-hidden px-6 text-center">
      <div aria-hidden style={SKY} className="pointer-events-none absolute inset-0 -z-20">
        <Aurora mask="top" height={0.5} curtains={3} className="absolute inset-0" />
      </div>
      {/* A dark veil keeps the text readable and the aurora quiet. */}
      <div aria-hidden className="pointer-events-none absolute inset-0 -z-10 bg-gradient-to-b from-background/80 via-background/55 to-background/85" />
      <div className="flex flex-col items-center gap-4">
        <MossMark className="size-24" />
        <div className="flex flex-col gap-1.5">
          <h2 className="text-2xl font-semibold tracking-tight">Welcome to Moss</h2>
          <p className="max-w-sm text-sm text-muted-foreground">
            A LaTeX editor that runs on your PC, syncs with Overleaf, and lets your AI agents help.
          </p>
        </div>
      </div>

      <Button size="lg" className="px-4" disabled={busy} onClick={onAdd}>
        <Plus data-icon="inline-start" />
        Add project
      </Button>

      <ul className="flex flex-wrap justify-center gap-x-6 gap-y-2 text-xs text-muted-foreground">
        <li className="flex items-center gap-1.5">
          <FolderInput className="size-3.5" />
          Start new or import a folder or zip
        </li>
        <li className="flex items-center gap-1.5">
          <Cloud className="size-3.5" />
          Pull a project from Overleaf
        </li>
        <li className="flex items-center gap-1.5">
          <Bot className="size-3.5" />
          Connect an agent in Agents
        </li>
      </ul>

      <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
        {tex === null ? (
          "Checking LaTeX…"
        ) : tex.found && tex.install ? (
          <>
            <CheckCircle2 className="size-3.5 text-emerald-400" />
            {tex.install.flavor} is ready to compile
          </>
        ) : (
          <span className="flex items-center gap-1.5 text-warning">
            <AlertTriangle className="size-3.5 shrink-0" />
            {tex.problem ?? "LaTeX was not found"}
            <Button variant="link" size="xs" className="px-1" onClick={onSetup}>
              Set up LaTeX
            </Button>
          </span>
        )}
      </p>
    </div>
  );
}
