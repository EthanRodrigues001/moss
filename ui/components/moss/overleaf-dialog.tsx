"use client";

import { useCallback, useEffect, useState } from "react";
import {
  CheckCircle2,
  Globe,
  Cloud,
  Download,
  FolderOpen,
  Loader2,
  LogOut,
  ShieldCheck,
  XCircle,
} from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { api } from "@/lib/moss-api";
import type {
  ChromePairing,
  OverleafStatus,
  ProjectSummary,
  RemoteProject,
  WriteTestReport,
} from "@/lib/types";

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Projects already in the Workspace, to mark Overleaf projects that were pulled before. */
  localProjects: ProjectSummary[];
  /** Pull one Overleaf project into a new Overleaf-tagged Project. */
  onPull: (project: RemoteProject) => Promise<boolean>;
};

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

function formatDate(iso: string | null) {
  if (!iso) return "";
  const d = new Date(iso);
  return Number.isNaN(d.getTime())
    ? ""
    : d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

export function OverleafDialog({ open, onOpenChange, localProjects, onPull }: Props) {
  const [status, setStatus] = useState<OverleafStatus | null>(null);
  const [waiting, setWaiting] = useState(false);
  const [showPaste, setShowPaste] = useState(false);
  const [pasted, setPasted] = useState("");
  const [connecting, setConnecting] = useState(false);
  const [pairing, setPairing] = useState<ChromePairing | null>(null);

  const [projects, setProjects] = useState<RemoteProject[] | null>(null);
  const [filter, setFilter] = useState("");
  const [pulling, setPulling] = useState<string | null>(null);
  const [testing, setTesting] = useState<string | null>(null);
  const [test, setTest] = useState<{ projectName: string; report: WriteTestReport } | null>(null);

  const loadProjects = useCallback(async () => {
    setProjects(null);
    try {
      setProjects(await api.overleafProjects());
    } catch (e) {
      setProjects([]);
      toast.error(`Could not load your Overleaf projects: ${message(e)}`);
    }
  }, []);

  // Refresh the connection state each time the dialog opens.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    api
      .overleafStatus()
      .then((s) => {
        if (cancelled) return;
        setStatus(s);
        if (s.connected) void loadProjects();
      })
      .catch((e) => toast.error(message(e)));
    return () => {
      cancelled = true;
    };
  }, [open, loadProjects]);

  // The sign-in window reports back through events.
  useEffect(() => {
    if (!open) return;
    let off: (() => void) | undefined;
    let disposed = false;
    api
      .onOverleafLogin({
        connected: (s) => {
          setWaiting(false);
          setPairing(null);
          setStatus(s);
          toast.success("Signed in to Overleaf");
          void loadProjects();
        },
        closed: () => setWaiting(false),
      })
      .then((fn) => {
        if (disposed) fn();
        else off = fn;
      });
    return () => {
      disposed = true;
      off?.();
    };
  }, [open, loadProjects]);

  async function signIn() {
    setWaiting(true);
    try {
      await api.overleafOpenLogin();
    } catch (e) {
      setWaiting(false);
      toast.error(message(e));
    }
  }

  async function startChrome() {
    try {
      setPairing(await api.overleafStartChromePairing());
    } catch (e) {
      toast.error(message(e));
    }
  }

  async function cancelChrome() {
    setPairing(null);
    await api.overleafStopChromePairing().catch(() => {});
  }

  async function connectPasted() {
    setConnecting(true);
    try {
      const s = await api.overleafConnectPasted(pasted);
      setStatus(s);
      setPasted("");
      setShowPaste(false);
      toast.success("Signed in to Overleaf");
      void loadProjects();
    } catch (e) {
      toast.error(message(e));
    } finally {
      setConnecting(false);
    }
  }

  async function signOut() {
    await api.overleafSignOut();
    setStatus({ connected: false, email: null, host: status?.host ?? "www.overleaf.com" });
    setProjects(null);
    setTest(null);
  }

  async function pull(p: RemoteProject) {
    setPulling(p.id);
    try {
      if (await onPull(p)) onOpenChange(false);
    } finally {
      setPulling(null);
    }
  }

  async function runTest(p: RemoteProject) {
    setTesting(p.id);
    setTest(null);
    try {
      setTest({ projectName: p.name, report: await api.overleafWriteTest(p.id) });
    } catch (e) {
      toast.error(message(e));
    } finally {
      setTesting(null);
    }
  }

  const linked = new Set(
    localProjects.flatMap((p) => (p.overleaf ? [p.overleaf.project_id] : [])),
  );
  const shown = (projects ?? []).filter((p) =>
    p.name.toLowerCase().includes(filter.trim().toLowerCase()),
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="overflow-hidden sm:max-w-xl [&>*]:min-w-0">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Cloud className="size-4" /> Overleaf
          </DialogTitle>
          <DialogDescription>
            {status?.connected
              ? `Signed in${status.email ? ` as ${status.email}` : ""}. Importing makes a new Overleaf-tagged project on this PC.`
              : "Sign in to import your Overleaf projects. Moss never sees or stores your password."}
          </DialogDescription>
        </DialogHeader>

        {status === null ? (
          <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground">
            <Loader2 className="size-4 animate-spin" /> Checking your Overleaf session…
          </div>
        ) : !status.connected ? (
          <div className="flex flex-col gap-3">
            <Button onClick={() => void signIn()} disabled={waiting}>
              {waiting ? (
                <Loader2 data-icon="inline-start" className="animate-spin" />
              ) : (
                <ShieldCheck data-icon="inline-start" />
              )}
              {waiting ? "Waiting for you to sign in…" : "Sign in with Overleaf"}
            </Button>
            {pairing ? (
              <ChromePanel pairing={pairing} onCancel={() => void cancelChrome()} />
            ) : (
              <Button variant="outline" onClick={() => void startChrome()}>
                <Globe data-icon="inline-start" />
                Connect from Chrome (uses your existing sign-in)
              </Button>
            )}
            <p className="text-xs text-muted-foreground">
              A separate window opens Overleaf&apos;s own sign-in page. This uses Overleaf&apos;s
              private web interface because the free plan has no official sync, so it is
              unofficial and Overleaf could change it.
            </p>

            <button
              type="button"
              onClick={() => setShowPaste((v) => !v)}
              className="self-start text-xs text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
            >
              Sign-in window not working? Paste your session cookie instead
            </button>
            {showPaste && (
              <div className="flex flex-col gap-2 rounded-lg border bg-card p-3">
                <p className="text-xs text-muted-foreground">
                  In your normal browser, signed in to Overleaf: press F12, open Application (or
                  Storage), then Cookies, then www.overleaf.com, and copy the Value of{" "}
                  <code>overleaf_session2</code>.
                </p>
                <textarea
                  value={pasted}
                  onChange={(e) => setPasted(e.target.value)}
                  spellCheck={false}
                  rows={3}
                  placeholder="s%3A…"
                  aria-label="overleaf_session2 cookie value"
                  className="w-full resize-none rounded-lg border border-input bg-background p-2 font-mono text-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
                />
                <Button
                  size="sm"
                  className="self-end"
                  onClick={() => void connectPasted()}
                  disabled={connecting || !pasted.trim()}
                >
                  {connecting && <Loader2 data-icon="inline-start" className="animate-spin" />}
                  Connect
                </Button>
              </div>
            )}
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            <div className="flex items-center gap-2">
              <Input
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
                placeholder="Filter your Overleaf projects"
                aria-label="Filter Overleaf projects"
              />
              <Button variant="ghost" size="sm" onClick={() => void signOut()}>
                <LogOut data-icon="inline-start" />
                Sign out
              </Button>
            </div>

            {/* A plain scroller, not ScrollArea: its content wrapper grows to the widest row, so a long
                title pushed the buttons out of the dialog instead of truncating. */}
            <div className="h-72 overflow-y-auto overflow-x-hidden rounded-lg border">
              {projects === null ? (
                <div className="flex items-center gap-2 p-4 text-sm text-muted-foreground">
                  <Loader2 className="size-4 animate-spin" /> Loading your projects…
                </div>
              ) : shown.length === 0 ? (
                <p className="p-4 text-sm text-muted-foreground">
                  {projects.length === 0
                    ? "No projects found on this Overleaf account."
                    : "No project matches that filter."}
                </p>
              ) : (
                <ul className="divide-y">
                  {shown.map((p) => (
                    <li key={p.id} className="flex items-center gap-3 px-3 py-2.5">
                      <div className="min-w-0 flex-1">
                        <div className="flex min-w-0 items-center gap-2">
                          <span className="truncate text-sm font-medium" title={p.name}>
                            {p.name}
                          </span>
                          {linked.has(p.id) && (
                            <Badge variant="outline" className="shrink-0 text-muted-foreground">
                              On this PC
                            </Badge>
                          )}
                        </div>
                        <div className="truncate text-xs text-muted-foreground">
                          {[p.owner, formatDate(p.last_updated)].filter(Boolean).join(" · ")}
                        </div>
                      </div>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="shrink-0"
                        onClick={() => void runTest(p)}
                        disabled={testing !== null || pulling !== null}
                        title="Uploads a tiny file to this project, then deletes it"
                      >
                        {testing === p.id ? (
                          <Loader2 data-icon="inline-start" className="animate-spin" />
                        ) : (
                          <ShieldCheck data-icon="inline-start" />
                        )}
                        Test write
                      </Button>
                      <Button
                        size="sm"
                        className="shrink-0"
                        onClick={() => void pull(p)}
                        disabled={pulling !== null || testing !== null}
                      >
                        {pulling === p.id ? (
                          <Loader2 data-icon="inline-start" className="animate-spin" />
                        ) : (
                          <Download data-icon="inline-start" />
                        )}
                        {linked.has(p.id) ? "Import again" : "Import"}
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
            </div>

            {test && <TestResult projectName={test.projectName} report={test.report} />}
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

function TestResult({ projectName, report }: { projectName: string; report: WriteTestReport }) {
  return (
    <div className="rounded-lg border bg-card p-3">
      <div className="mb-2 flex items-center gap-2 text-sm font-medium">
        {report.ok ? (
          <CheckCircle2 className="size-4 text-emerald-400" />
        ) : (
          <XCircle className="size-4 text-destructive" />
        )}
        Write test on “{projectName}”: {report.ok ? "passed" : "failed"}
      </div>
      <ul className="flex flex-col gap-1 text-xs">
        {report.steps.map((s) => (
          <li key={s.name} className="flex items-start gap-2">
            {s.ok ? (
              <CheckCircle2 className="mt-0.5 size-3.5 shrink-0 text-emerald-400" />
            ) : (
              <XCircle className="mt-0.5 size-3.5 shrink-0 text-destructive" />
            )}
            <span className="min-w-0">
              <span className="font-medium">{s.name}</span>{" "}
              <span className="text-muted-foreground">
                {s.detail} ({s.ms} ms)
              </span>
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function ChromePanel({ pairing, onCancel }: { pairing: ChromePairing; onCancel: () => void }) {
  return (
    <div className="flex flex-col gap-3 rounded-lg border bg-card p-3">
      <div className="flex items-center justify-between gap-3">
        <div>
          <div className="text-xs text-muted-foreground">Type this code in the extension</div>
          <div className="font-mono text-2xl font-semibold tracking-[0.35em]">{pairing.code}</div>
        </div>
        <Button variant="ghost" size="sm" onClick={onCancel}>
          Cancel
        </Button>
      </div>
      <ol className="list-decimal space-y-1 pl-4 text-xs text-muted-foreground">
        <li>
          Once only: in Chrome open <code>chrome://extensions</code>, turn on Developer mode, click
          Load unpacked and pick the Moss Connector folder.
        </li>
        <li>Make sure you are signed in to Overleaf in that Chrome.</li>
        <li>Click the Moss Connector icon, enter the code, and press the button.</li>
      </ol>
      {pairing.extension_dir && (
        <Button
          variant="outline"
          size="sm"
          className="self-start"
          onClick={() => void api.revealInExplorer(pairing.extension_dir!)}
        >
          <FolderOpen data-icon="inline-start" />
          Open the extension folder
        </Button>
      )}
      <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <Loader2 className="size-3.5 animate-spin" /> Waiting for Chrome… (the code works for 3
        minutes)
      </p>
    </div>
  );
}
