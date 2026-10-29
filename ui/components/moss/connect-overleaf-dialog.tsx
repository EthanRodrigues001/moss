"use client";

import { useEffect, useState } from "react";
import { CloudUpload, FlaskConical, Link2, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { api } from "@/lib/moss-api";
import type { OverleafStatus, RemoteProject } from "@/lib/types";

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** The Local Project being connected (its folder name and display name). */
  project: string;
  projectName: string;
  /** The link was made: reload the Project's state. */
  onLinked: () => void;
  /** Send the user to the Overleaf sign-in dialog. */
  onSignIn: () => void;
};

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

type Step = "choose" | "pick";

/** Turn a Local Project into an Overleaf one: new Overleaf project, an existing one, or a practice copy. */
export function ConnectOverleafDialog({ open, onOpenChange, project, projectName, onLinked, onSignIn }: Props) {
  const [step, setStep] = useState<Step>("choose");
  const [status, setStatus] = useState<OverleafStatus | null>(null);
  const [projects, setProjects] = useState<RemoteProject[] | null>(null);
  const [filter, setFilter] = useState("");
  const [busy, setBusy] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    api.overleafStatus().then((s) => !cancelled && setStatus(s), () => !cancelled && setStatus(null));
    return () => {
      cancelled = true;
    };
  }, [open]);

  const finish = (text: string) => {
    toast.success(text);
    onOpenChange(false);
    setStep("choose");
    onLinked();
  };

  async function run(key: string, work: () => Promise<unknown>, done: string) {
    setBusy(key);
    try {
      await work();
      finish(done);
    } catch (e) {
      toast.error(message(e));
    } finally {
      setBusy(null);
    }
  }

  async function openPicker() {
    setStep("pick");
    setProjects(null);
    try {
      setProjects(await api.overleafProjects());
    } catch (e) {
      setProjects([]);
      toast.error(`Could not load your Overleaf projects: ${message(e)}`);
    }
  }

  const connected = Boolean(status?.connected);
  const shown = (projects ?? []).filter((p) => p.name.toLowerCase().includes(filter.toLowerCase()));

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        onOpenChange(o);
        if (!o) setStep("choose");
      }}
    >
      <DialogContent className="sm:max-w-lg" data-testid="connect-dialog">
        <DialogHeader>
          <DialogTitle>Connect “{projectName}” to Overleaf</DialogTitle>
          <DialogDescription>
            After this the project can Pull, Push and Sync. Your files stay on this PC.
          </DialogDescription>
        </DialogHeader>

        {step === "choose" && (
          <div className="grid gap-2">
            {!connected && (
              <p className="rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-300">
                You are not signed in to Overleaf.{" "}
                <button type="button" className="font-semibold underline" onClick={() => { onOpenChange(false); onSignIn(); }}>
                  Sign in
                </button>{" "}
                to use the first two options.
              </p>
            )}
            <Choice
              icon={<CloudUpload className="size-5" />}
              title="Create a new Overleaf project from this one"
              text="Uploads these files as a new project on Overleaf and links them."
              disabled={!connected || busy !== null}
              busy={busy === "new"}
              onClick={() => void run("new", () => api.syncLinkNew(project), "Created the Overleaf project and linked it")}
            />
            <Choice
              icon={<Link2 className="size-5" />}
              title="Link to an Overleaf project that already exists"
              text="Pick one. Nothing is deleted; files that differ are shown for you to decide."
              disabled={!connected || busy !== null}
              onClick={() => void openPicker()}
            />
            <Choice
              icon={<FlaskConical className="size-5" />}
              title="Try it with a practice copy"
              text="Makes a folder on this PC that behaves like Overleaf. No account, no risk."
              disabled={busy !== null}
              busy={busy === "practice"}
              onClick={() => void run("practice", () => api.syncLinkPractice(project), "Linked to a practice copy")}
            />
          </div>
        )}

        {step === "pick" && (
          <div className="grid gap-3">
            <Input placeholder="Search your Overleaf projects" value={filter} onChange={(e) => setFilter(e.target.value)} />
            <ScrollArea className="h-64 rounded-lg border">
              {projects === null ? (
                <p className="flex items-center justify-center gap-2 p-6 text-sm text-muted-foreground">
                  <Loader2 className="size-4 animate-spin" />
                  Loading…
                </p>
              ) : shown.length === 0 ? (
                <p className="p-6 text-center text-sm text-muted-foreground">No projects found.</p>
              ) : (
                <ul>
                  {shown.map((p) => (
                    <li key={p.id}>
                      <button
                        type="button"
                        disabled={busy !== null}
                        onClick={() =>
                          void run(p.id, () => api.syncLinkExisting(project, p.id, p.name), `Linked to “${p.name}”`)
                        }
                        className="flex w-full items-center gap-2 px-3 py-2 text-left text-sm hover:bg-accent disabled:opacity-50"
                      >
                        <span className="min-w-0 flex-1 truncate font-medium">{p.name}</span>
                        {busy === p.id && <Loader2 className="size-4 animate-spin" />}
                        <span className="text-xs text-muted-foreground">{p.owner}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </ScrollArea>
            <div className="flex justify-between">
              <Button variant="ghost" onClick={() => setStep("choose")}>
                Back
              </Button>
            </div>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
}

function Choice({
  icon,
  title,
  text,
  disabled,
  busy,
  onClick,
}: {
  icon: React.ReactNode;
  title: string;
  text: string;
  disabled: boolean;
  busy?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onClick}
      className="flex items-start gap-3 rounded-xl border bg-card px-3.5 py-3 text-left transition-colors hover:bg-accent disabled:opacity-50 disabled:hover:bg-card"
    >
      <span className="mt-0.5 text-primary">{busy ? <Loader2 className="size-5 animate-spin" /> : icon}</span>
      <span>
        <span className="block text-sm font-semibold">{title}</span>
        <span className="block text-xs text-muted-foreground">{text}</span>
      </span>
    </button>
  );
}
