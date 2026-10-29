"use client";

import { useEffect, useState } from "react";
import { Check, Copy } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { api } from "@/lib/moss-api";
import type { AgentSetup } from "@/lib/types";

function CopyBlock({ label, text }: { label: string; text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">{label}</h3>
        <Button
          variant="ghost"
          size="xs"
          onClick={() => {
            void navigator.clipboard.writeText(text);
            setCopied(true);
            setTimeout(() => setCopied(false), 1500);
          }}
        >
          {copied ? <Check data-icon="inline-start" /> : <Copy data-icon="inline-start" />}
          {copied ? "Copied" : "Copy"}
        </Button>
      </div>
      <pre className="max-h-40 overflow-auto rounded-lg border bg-muted px-3 py-2 font-mono text-xs break-all whitespace-pre-wrap">
        {text}
      </pre>
    </div>
  );
}

/** How to connect an AI client to `moss-mcp`. */
export function AgentsDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [setup, setSetup] = useState<AgentSetup | null>(null);

  useEffect(() => {
    if (open) api.agentSetup().then(setSetup).catch(() => setSetup(null));
  }, [open]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Connect an agent</DialogTitle>
          <DialogDescription>
            Any MCP client (Claude, Cursor, Codex…) can then read, edit, compile and undo in your
            projects. Deleting files and turning on shell-escape wait for your click here.
          </DialogDescription>
        </DialogHeader>

        {setup && (
          <div className="flex flex-col gap-4">
            {!setup.command && (
              <p className="text-sm text-warning">
                <code>moss-mcp.exe</code> was not found next to the app. Build it with{" "}
                <code>cargo build -p moss-mcp</code> and the path below will be filled in.
              </p>
            )}
            <CopyBlock label="Claude Code" text={setup.claude_code} />
            <CopyBlock label="Claude Desktop, Cursor and others (MCP settings JSON)" text={setup.config} />
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}
