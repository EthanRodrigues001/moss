"use client";

import { useEffect, useMemo, useState } from "react";
import { Check, CheckCircle2, Copy, RefreshCw, Search, TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { api } from "@/lib/moss-api";
import { plural, relativeTime } from "@/lib/time";
import type { AgentSetup, McpClient, McpStatus, McpTool } from "@/lib/types";
import { cn } from "@/lib/utils";
import { SettingsGroup, SettingsRow } from "./settings-ui";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

function CopyBlock({ label, text }: { label: string; text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="flex flex-col gap-1.5 px-4 py-3">
      <div className="flex items-center justify-between">
        <h4 className="text-sm font-medium">{label}</h4>
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

function Dot({ tone }: { tone: "ok" | "warn" | "bad" | "idle" }) {
  const color = {
    ok: "bg-emerald-400",
    warn: "bg-amber-400",
    bad: "bg-destructive",
    idle: "bg-muted-foreground/50",
  }[tone];
  return <span className={cn("size-2 shrink-0 rounded-full", color)} aria-hidden />;
}

/** How one AI program stands: set up and current, set up but pointing elsewhere, found, or absent. */
function clientState(c: McpClient): { badge: React.ReactNode; text: React.ReactNode } {
  if (c.configured && c.current) {
    return {
      badge: <Badge variant="outline" className="border-emerald-500/40 text-emerald-400">Connected</Badge>,
      text: "Set up, and it runs this copy of Moss.",
    };
  }
  if (c.configured) {
    return {
      badge: <Badge variant="outline" className="border-amber-500/40 text-amber-400">Old path</Badge>,
      text: (
        <>
          Set up, but it runs <code className="font-mono break-all">{c.command ?? "another copy"}</code>. Run the
          command below again to point it here.
        </>
      ),
    };
  }
  if (c.installed) {
    return { badge: <Badge variant="secondary">Not set up</Badge>, text: "Found on this PC. Use one of the commands below." };
  }
  return { badge: <Badge variant="outline">Not found</Badge>, text: "Not found on this PC." };
}

function ToolBadges({ tool }: { tool: McpTool }) {
  return (
    <span className="mt-1.5 flex flex-wrap gap-1">
      {tool.read_only ? (
        <Badge variant="outline">Read-only</Badge>
      ) : tool.destructive ? (
        <Badge variant="destructive">Can overwrite or delete</Badge>
      ) : (
        <Badge variant="secondary">Makes changes</Badge>
      )}
      {tool.needs_approval && (
        <Badge variant="outline" className="border-amber-500/40 text-amber-400">
          Asks you first
        </Badge>
      )}
      {tool.online && <Badge variant="outline">Uses Overleaf</Badge>}
    </span>
  );
}

/** Settings page for the MCP server: its status, who is connected, what agents did, and every tool. */
export function McpSettings() {
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [setup, setSetup] = useState<AgentSetup | null>(null);
  const [checking, setChecking] = useState(true);
  const [query, setQuery] = useState("");

  useEffect(() => {
    let alive = true;
    api
      .mcpStatus()
      .then((s) => alive && setStatus(s))
      .catch((e) => alive && toast.error(message(e)))
      .finally(() => alive && setChecking(false));
    api
      .agentSetup()
      .then((s) => alive && setSetup(s))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);

  async function recheck() {
    setChecking(true);
    try {
      setStatus(await api.mcpStatus());
    } catch (e) {
      toast.error(message(e));
    } finally {
      setChecking(false);
    }
  }

  async function setEnabled(names: string[], enabled: boolean) {
    try {
      const disabled = await api.mcpSetToolsEnabled(names, enabled);
      setStatus((s) => (s ? { ...s, disabled } : s));
    } catch (e) {
      toast.error(message(e));
    }
  }

  const off = useMemo(() => new Set(status?.disabled ?? []), [status?.disabled]);
  const groups = useMemo(() => {
    const q = query.trim().toLowerCase();
    const byGroup = new Map<string, McpTool[]>();
    for (const t of status?.tools ?? []) {
      if (q && !`${t.name} ${t.title} ${t.description} ${t.group}`.toLowerCase().includes(q)) continue;
      byGroup.set(t.group, [...(byGroup.get(t.group) ?? []), t]);
    }
    return [...byGroup.entries()];
  }, [status?.tools, query]);

  const toolCount = status?.tools.length ?? 0;
  const state = status?.state;
  const statusTitle =
    state === "ready" ? "Ready" : state === "missing" ? "Not found" : state === "broken" ? "Not responding" : "Checking…";
  const statusText =
    state === "ready"
      ? `moss-mcp ${status?.version ?? ""} · ${plural(toolCount, "tool")}${off.size > 0 ? ` · ${off.size} switched off` : ""}`
      : state === "missing"
        ? "moss-mcp.exe is not next to the app. Build it with `cargo build -p moss-mcp`, then check again."
        : (status?.problem ?? "Looking for moss-mcp…");

  return (
    <div className="flex flex-col gap-8">
      <SettingsGroup title="Server">
        <SettingsRow
          title={
            <span className="flex items-center gap-2">
              <Dot tone={state === "ready" ? "ok" : state === "missing" ? "warn" : state === "broken" ? "bad" : "idle"} />
              {statusTitle}
            </span>
          }
          description={statusText}
        >
          <Button variant="outline" size="sm" disabled={checking} onClick={() => void recheck()}>
            <RefreshCw data-icon="inline-start" className={cn(checking && "animate-spin")} />
            Check again
          </Button>
        </SettingsRow>
        {status?.exe && (
          <SettingsRow
            title="Program"
            description={<span className="font-mono break-all">{status.exe}</span>}
          />
        )}
        {status && (
          <SettingsRow
            title="Projects folder"
            description={<span className="font-mono break-all">{status.workspace}</span>}
          />
        )}
        {status?.needs.map((n) => (
          <SettingsRow key={n.name} title={n.name} description={<span className="break-all">{n.detail}</span>}>
            {n.ok ? (
              <CheckCircle2 className="size-4 text-emerald-400" aria-label="Found" />
            ) : (
              <TriangleAlert className="size-4 text-amber-400" aria-label="Missing" />
            )}
          </SettingsRow>
        ))}
        {status && status.pending_approvals > 0 && (
          <SettingsRow
            title="Waiting for you"
            description={`${plural(status.pending_approvals, "request")} from an agent need your click in the app.`}
          >
            <TriangleAlert className="size-4 text-amber-400" aria-label="Waiting" />
          </SettingsRow>
        )}
      </SettingsGroup>

      <SettingsGroup title="Connect an agent">
        {status?.clients.map((c) => {
          const s = clientState(c);
          return (
            <SettingsRow
              key={c.name}
              title={c.name}
              description={
                <>
                  {s.text}
                  {c.installed && <span className="mt-0.5 block font-mono break-all opacity-70">{c.config_file}</span>}
                </>
              }
            >
              {s.badge}
            </SettingsRow>
          );
        })}
        {setup && !setup.command && (
          <p className="px-4 py-3 text-xs text-amber-400">
            <code>moss-mcp.exe</code> was not found next to the app, so the paths below are placeholders.
          </p>
        )}
        {setup && <CopyBlock label="Claude Code (run in a terminal)" text={setup.claude_code} />}
        {setup && <CopyBlock label="Claude Desktop, Cursor and others (MCP settings JSON)" text={setup.config} />}
      </SettingsGroup>

      <SettingsGroup title="Recent agent activity">
        {status && status.activity.length === 0 && (
          <SettingsRow title="Nothing yet" description="What an agent changes in your projects shows up here." />
        )}
        {status?.activity.map((a, i) => (
          <SettingsRow
            key={`${a.at}-${i}`}
            title={status.tools.find((t) => t.name === a.action)?.title ?? a.action}
            description={
              <span className="break-all">
                {a.project}
                {a.detail && ` · ${a.detail}`}
              </span>
            }
          >
            <span className="text-xs text-muted-foreground">{relativeTime(a.at)}</span>
          </SettingsRow>
        ))}
      </SettingsGroup>

      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-1 px-1">
          <h2 className="text-base font-medium">Tools</h2>
          <p className="text-xs text-muted-foreground">
            Agents only see the tools that are on. A change applies the next time an agent starts Moss, so restart
            the agent if it is already running.
          </p>
        </div>
        <div className="relative">
          <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={`Search ${plural(toolCount, "tool")}`}
            className="pl-9"
            aria-label="Search tools"
          />
        </div>
        {status && groups.length === 0 && (
          <p className="px-1 text-sm text-muted-foreground">
            {toolCount === 0 ? "No tools to show until moss-mcp is found." : "No tool matches that search."}
          </p>
        )}
        {groups.map(([group, tools]) => {
          const names = tools.map((t) => t.name);
          const allOn = names.every((n) => !off.has(n));
          return (
            <SettingsGroup
              key={group}
              title={`${group} · ${tools.length}`}
              aside={
                <Button variant="ghost" size="xs" onClick={() => void setEnabled(names, !allOn)}>
                  {allOn ? "Switch all off" : "Switch all on"}
                </Button>
              }
            >
              {tools.map((t) => (
                <SettingsRow
                  key={t.name}
                  className={cn(off.has(t.name) && "opacity-60")}
                  title={
                    <span className="flex flex-wrap items-baseline gap-x-2">
                      {t.title}
                      <code className="font-mono text-xs font-normal text-muted-foreground">{t.name}</code>
                    </span>
                  }
                  description={
                    <>
                      {t.description}
                      <ToolBadges tool={t} />
                    </>
                  }
                >
                  <Switch
                    checked={!off.has(t.name)}
                    onCheckedChange={(on) => void setEnabled([t.name], on)}
                    aria-label={`${t.title}: on or off`}
                  />
                </SettingsRow>
              ))}
            </SettingsGroup>
          );
        })}
      </div>
    </div>
  );
}
