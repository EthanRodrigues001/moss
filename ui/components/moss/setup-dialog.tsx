"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { AlertTriangle, Bot, CheckCircle2, Circle, Cloud, Loader2, XCircle } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { api } from "@/lib/moss-api";
import type { ProjectSummary, SelfTest, SetupProgress, SetupReport, TexStatus } from "@/lib/types";
import { MossMark } from "./moss-mark";

type Step = "check" | "installing" | "test" | "done";

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onTexChange: (tex: TexStatus) => void;
  onTour: (project: ProjectSummary) => void;
  onOverleaf: () => void;
  onAgents: () => void;
};

const mb = (bytes: number) => `${Math.round(bytes / 1_000_000)} MB`;
const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

const PHASE_LABEL: Record<SetupProgress["phase"], string> = {
  download: "Downloading",
  verify: "Checking the download",
  install: "Installing",
  configure: "Setting up",
};

/** First-run setup: check this PC, install LaTeX in one click, prove it with a test compile. */
export function SetupDialog(props: Props) {
  const { open, onOpenChange } = props;
  // Closing counts as "not now": the dialog stays out of the way but Settings can reopen it.
  const close = (next: boolean) => {
    if (!next) void api.setupMarkOnboarded();
    onOpenChange(next);
  };
  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="gap-0 overflow-hidden p-0 sm:max-w-lg" showCloseButton={false}>
        <DialogTitle className="sr-only">Set up LaTeX</DialogTitle>
        <DialogDescription className="sr-only">Check this PC and install what Moss needs to compile.</DialogDescription>
        {/* Mounted only while open, so it always starts with a fresh check. */}
        {open && <Body {...props} close={() => close(false)} />}
      </DialogContent>
    </Dialog>
  );
}

function Body({ onTexChange, onTour, onOverleaf, onAgents, close }: Props & { close: () => void }) {
  const [step, setStep] = useState<Step>("check");
  const [report, setReport] = useState<SetupReport | null>(null);
  const [warm, setWarm] = useState(true);
  const [progress, setProgress] = useState<Record<string, SetupProgress>>({});
  const [error, setError] = useState<string | null>(null);
  const [test, setTest] = useState<SelfTest | null>(null);
  const [working, setWorking] = useState(false);
  const started = useRef(false);

  const look = useCallback(async () => {
    setReport(null);
    try {
      setReport(await api.setupCheck());
    } catch (e) {
      setError(msg(e));
    }
  }, []);

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    void look();
  }, [look]);

  useEffect(() => {
    let off: (() => void) | undefined;
    let gone = false;
    void api.onSetupProgress((p) => setProgress((prev) => ({ ...prev, [p.part]: p }))).then((u) => {
      if (gone) u();
      else off = u;
    });
    return () => {
      gone = true;
      off?.();
    };
  }, []);

  const runTest = useCallback(async () => {
    setStep("test");
    setTest(null);
    try {
      setTest(await api.setupSelfTest());
    } catch (e) {
      setTest({ ok: false, seconds: 0, pages: null, problem: msg(e) });
    }
  }, []);

  async function install() {
    if (!report) return;
    setError(null);
    setProgress({});
    setStep("installing");
    try {
      const tex = await api.setupInstall(
        report.missing.map((m) => m.id),
        warm,
      );
      onTexChange(tex);
      await runTest();
    } catch (e) {
      const text = msg(e);
      if (text !== "cancelled") setError(text);
      setStep("check");
      void look();
    }
  }

  async function finish(openTour: boolean) {
    setWorking(true);
    try {
      await api.setupMarkOnboarded();
      if (openTour) onTour(await api.setupCreateTour());
      close();
    } catch (e) {
      toast.error(msg(e));
    } finally {
      setWorking(false);
    }
  }

  return (
    <div className="flex flex-col">
      {/* Banner: the logo's own artwork (dark base, tilted page, blue sigma) with the logo and title on it. */}
      <div className="relative isolate flex items-center gap-4 overflow-hidden border-b bg-[#1d1d21] px-6 py-7 text-white">
        {/* eslint-disable-next-line @next/next/no-img-element */}
        <img src="/setup-banner.svg" alt="" aria-hidden draggable={false} className="pointer-events-none absolute inset-0 -z-10 size-full object-cover object-right" />
        <MossMark className="size-16" />
        <div>
          <p className="text-xl font-semibold tracking-tight">Set up LaTeX</p>
          <p className="text-sm text-white/70">Moss compiles with a LaTeX install on this PC.</p>
        </div>
      </div>

      <div className="min-h-56 px-5 py-4">
        {step === "check" && (
          <CheckStep report={report} error={error} warm={warm} onWarm={setWarm} />
        )}
        {step === "installing" && <InstallStep report={report} progress={progress} />}
        {step === "test" && <TestStep test={test} />}
        {step === "done" && <DoneStep onOverleaf={onOverleaf} onAgents={onAgents} />}
      </div>

      <div className="flex items-center justify-end gap-2 border-t px-5 py-3">
        {step === "check" && (
          <>
            <Button variant="ghost" onClick={close}>
              Not now
            </Button>
            {report && report.missing.length > 0 && !report.foreign_tex && (
              <Button onClick={() => void install()}>
                Install ({mb(report.missing.reduce((n, m) => n + m.size, 0))})
              </Button>
            )}
            {report && report.missing.length === 0 && <Button onClick={() => void runTest()}>Test compile</Button>}
          </>
        )}
        {step === "installing" && (
          <Button variant="outline" onClick={() => void api.setupCancel()}>
            Cancel
          </Button>
        )}
        {step === "test" && test && !test.ok && (
          <>
            <Button variant="ghost" onClick={close}>
              Close
            </Button>
            <Button variant="outline" onClick={() => void runTest()}>
              Try again
            </Button>
            <Button variant="outline" onClick={() => { setStep("check"); void look(); }}>
              Check this PC
            </Button>
          </>
        )}
        {step === "test" && test?.ok && <Button onClick={() => setStep("done")}>Continue</Button>}
        {step === "done" && (
          <>
            <Button variant="ghost" disabled={working} onClick={() => void finish(false)}>
              Finish
            </Button>
            <Button disabled={working} onClick={() => void finish(true)}>
              Open the Moss tour
            </Button>
          </>
        )}
      </div>
    </div>
  );
}

function Item({ ok, label, detail }: { ok: boolean | null; label: string; detail?: string }) {
  return (
    <li className="flex items-start gap-2.5 text-sm">
      {ok === null ? (
        <Circle className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
      ) : ok ? (
        <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-emerald-400" />
      ) : (
        <XCircle className="mt-0.5 size-4 shrink-0 text-warning" />
      )}
      <span className="min-w-0">
        {label}
        {detail && <span className="block truncate text-xs text-muted-foreground">{detail}</span>}
      </span>
    </li>
  );
}

function CheckStep({
  report,
  error,
  warm,
  onWarm,
}: {
  report: SetupReport | null;
  error: string | null;
  warm: boolean;
  onWarm: (v: boolean) => void;
}) {
  if (!report) {
    return error ? (
      <p className="flex items-start gap-2 text-sm text-warning">
        <AlertTriangle className="mt-0.5 size-4 shrink-0" />
        {error}
      </p>
    ) : (
      <p className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        Looking at this PC…
      </p>
    );
  }
  const installable = report.missing.length > 0 && !report.foreign_tex;
  return (
    <div className="flex flex-col gap-4">
      <ul className="flex flex-col gap-2.5">
        <Item
          ok={report.tex !== null}
          label={report.tex ? `${report.tex.flavor} (TeX distribution)` : "TeX distribution"}
          detail={report.tex?.bin_dir}
        />
        <Item ok={report.perl !== null} label="Perl (needed by latexmk)" detail={report.perl ?? undefined} />
        <Item ok={report.synctex} label="SyncTeX (click between code and PDF)" />
        <Item ok={report.pdftoppm} label="Page images (for agents to see the PDF)" />
        {report.tex?.flavor === "MiKTeX" && (
          <Item
            ok={report.auto_install === "yes"}
            label="MiKTeX installs missing packages by itself"
            detail={report.auto_install === "yes" ? undefined : "Compiles may stop to ask. Install below turns it on."}
          />
        )}
      </ul>
      {error && (
        <p className="flex items-start gap-2 text-sm text-warning">
          <AlertTriangle className="mt-0.5 size-4 shrink-0" />
          {error}
        </p>
      )}
      {installable && (
        <div className="flex flex-col gap-2 rounded-lg border bg-card/50 p-3 text-sm">
          <p className="font-medium">One click installs, for you only (no admin prompt):</p>
          <ul className="flex flex-col gap-1 text-xs text-muted-foreground">
            {report.missing.map((m) => (
              <li key={m.id}>
                {m.name}, {mb(m.size)}
              </li>
            ))}
          </ul>
          <label className="flex items-center gap-2 text-xs">
            <input type="checkbox" checked={warm} onChange={(e) => onWarm(e.target.checked)} className="accent-primary" />
            Also pre-install common packages, so your first compile is quick
          </label>
        </div>
      )}
      {report.foreign_tex && report.perl === null && (
        <p className="text-xs text-muted-foreground">
          Moss does not install over TeX Live. Install Perl yourself, or choose its folder in Settings.
        </p>
      )}
      {report.missing.length === 0 && (
        <p className="text-sm text-muted-foreground">Everything Moss needs is here. A quick test compile comes next.</p>
      )}
    </div>
  );
}

function InstallStep({ report, progress }: { report: SetupReport | null; progress: Record<string, SetupProgress> }) {
  return (
    <ul className="flex flex-col gap-4">
      {(report?.missing ?? []).map((m) => {
        const p = progress[m.id];
        const pct = p && p.total ? Math.min(100, Math.round((p.done / p.total) * 100)) : null;
        return (
          <li key={m.id} className="flex flex-col gap-1.5">
            <div className="flex items-center justify-between gap-3 text-sm">
              <span className="truncate">{m.name}</span>
              <span className="shrink-0 text-xs text-muted-foreground">
                {p ? `${PHASE_LABEL[p.phase]}${pct !== null && p.phase !== "verify" ? ` ${pct}%` : "…"}` : "Waiting"}
              </span>
            </div>
            <div className="h-1.5 overflow-hidden rounded-full bg-muted">
              <div
                className={pct === null && p ? "h-full w-1/3 animate-pulse rounded-full bg-primary" : "h-full rounded-full bg-primary transition-[width]"}
                style={pct !== null ? { width: `${pct}%` } : p ? undefined : { width: 0 }}
              />
            </div>
            {p?.note && <p className="text-xs text-muted-foreground">{p.note}</p>}
          </li>
        );
      })}
      <li className="text-xs text-muted-foreground">
        This can take several minutes. You can keep the window open and do something else; Cancel keeps what was
        downloaded so the next try carries on.
      </li>
    </ul>
  );
}

function TestStep({ test }: { test: SelfTest | null }) {
  if (!test)
    return (
      <p className="flex items-center gap-2 text-sm text-muted-foreground">
        <Loader2 className="size-4 animate-spin" />
        Compiling a small test document. The first run can take a minute while packages download.
      </p>
    );
  return test.ok ? (
    <p className="flex items-start gap-2 text-sm">
      <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-emerald-400" />
      <span>
        It works. A test document compiled in {test.seconds.toFixed(1)} s
        {test.pages ? ` (${test.pages} page${test.pages === 1 ? "" : "s"})` : ""}.
      </span>
    </p>
  ) : (
    <p className="flex items-start gap-2 text-sm text-warning">
      <AlertTriangle className="mt-0.5 size-4 shrink-0" />
      {test.problem ?? "The test compile failed."}
    </p>
  );
}

function DoneStep({ onOverleaf, onAgents }: { onOverleaf: () => void; onAgents: () => void }) {
  return (
    <div className="flex flex-col gap-4">
      <p className="text-sm">You are ready. The Moss tour is a short project that shows what the editor can do.</p>
      <div className="flex flex-col gap-2">
        <p className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Optional</p>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" onClick={onOverleaf}>
            <Cloud data-icon="inline-start" />
            Connect Overleaf
          </Button>
          <Button variant="outline" size="sm" onClick={onAgents}>
            <Bot data-icon="inline-start" />
            Connect an AI agent
          </Button>
        </div>
      </div>
    </div>
  );
}
