"use client";

import { useEffect, useState } from "react";
import { Download, FolderCog, Plug, SlidersHorizontal } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { api } from "@/lib/moss-api";
import type {
  Engine,
  ProjectPatch,
  ProjectSettings,
  ProjectSummary,
  SettingsView,
  TexStatus,
} from "@/lib/types";
import { cn } from "@/lib/utils";
import type { Updater } from "@/lib/use-updater";
import { McpSettings } from "./mcp-settings";
import { UpdatesSettings } from "./updates-settings";
import { SettingsGroup, SettingsRow } from "./settings-ui";

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

const ENGINES: { value: Engine; label: string }[] = [
  { value: "auto", label: "Automatic" },
  { value: "pdflatex", label: "pdfLaTeX" },
  { value: "xelatex", label: "XeLaTeX" },
  { value: "lualatex", label: "LuaLaTeX" },
];

export type SettingsPage = "general" | "project" | "mcp" | "updates";

const PAGES: { id: SettingsPage; label: string; icon: React.ComponentType<{ className?: string }> }[] = [
  { id: "general", label: "General", icon: SlidersHorizontal },
  { id: "project", label: "This project", icon: FolderCog },
  { id: "mcp", label: "MCP", icon: Plug },
  { id: "updates", label: "Updates", icon: Download },
];

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  page: SettingsPage;
  onPageChange: (page: SettingsPage) => void;
  project: ProjectSummary | null;
  tex: TexStatus | null;
  onTexChange: (tex: TexStatus) => void;
  updater: Updater;
  /** Opens first-run setup (check this PC, install LaTeX). */
  onSetup: () => void;
  /** Called after a per-project change so the project list can refresh. */
  onProjectChanged: () => void;
};

export function SettingsDialog({
  open,
  onOpenChange,
  page,
  onPageChange,
  project,
  tex,
  onTexChange,
  updater,
  onSetup,
  onProjectChanged,
}: Props) {
  const [settings, setSettings] = useState<SettingsView | null>(null);
  const [projectSettings, setProjectSettings] = useState<ProjectSettings | null>(null);
  const dirName = project?.dir_name ?? null;

  useEffect(() => {
    if (!open) return;
    api.settingsGet().then(setSettings).catch((e) => toast.error(message(e)));
    if (dirName) {
      api.projectSettingsGet(dirName).then(setProjectSettings).catch((e) => toast.error(message(e)));
    }
  }, [open, dirName]);

  async function chooseWorkspace() {
    try {
      const next = await api.settingsChooseWorkspace();
      if (next) setSettings(next);
    } catch (e) {
      toast.error(message(e));
    }
  }

  async function chooseTexDir(reset: boolean) {
    try {
      const next = reset ? await api.settingsResetTexDir() : await api.settingsChooseTexDir();
      if (!next) return;
      onTexChange(next);
      setSettings(await api.settingsGet());
    } catch (e) {
      toast.error(message(e));
    }
  }

  async function patchProject(patch: ProjectPatch) {
    if (!dirName) return;
    try {
      setProjectSettings(await api.projectSettingsSet(dirName, patch));
      onProjectChanged();
    } catch (e) {
      toast.error(message(e));
    }
  }

  const restartNeeded = settings !== null && settings.workspace !== settings.running_workspace;
  const current = PAGES.find((p) => p.id === page) ?? PAGES[0];

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="h-[85vh] gap-0 overflow-hidden p-0 sm:max-w-5xl">
        <div className="grid h-full min-h-0 grid-cols-[13rem_minmax(0,1fr)]">
          <nav aria-label="Settings sections" className="flex flex-col gap-1 border-r p-3">
            {PAGES.map(({ id, label, icon: Icon }) => (
              <button
                key={id}
                type="button"
                aria-current={id === page ? "page" : undefined}
                onClick={() => onPageChange(id)}
                className={cn(
                  "flex items-center gap-2.5 rounded-lg px-3 py-2 text-left text-sm text-muted-foreground transition-colors hover:bg-accent/60 hover:text-foreground",
                  id === page && "bg-accent text-foreground",
                )}
              >
                <Icon className="size-4" />
                {label}
              </button>
            ))}
          </nav>

          <div className="flex min-h-0 flex-col">
            <header className="flex items-center border-b px-6 py-3 pr-12">
              <DialogTitle className="flex items-center gap-2 text-sm">
                <span className="font-normal text-muted-foreground">Settings</span>
                <span className="text-muted-foreground">/</span>
                {current.label}
              </DialogTitle>
              <DialogDescription className="sr-only">
                Where your projects live, how they compile, and what AI agents can do.
              </DialogDescription>
            </header>

            <div className="min-h-0 flex-1 overflow-y-auto px-6 py-6">
              <div className="mx-auto flex max-w-3xl flex-col gap-8">
                {page === "general" && (
                  <>
                    <SettingsGroup title="Storage">
                      <SettingsRow
                        title="Projects folder"
                        description={
                          <>
                            <span className="font-mono break-all">{settings?.workspace ?? "…"}</span>
                            <span className={cn("mt-1 block", restartNeeded && "text-amber-400")}>
                              {restartNeeded
                                ? "Restart Moss to switch to this folder. Existing projects are not moved; copy them over if you want them there."
                                : "Every project is an ordinary folder inside it."}
                            </span>
                          </>
                        }
                      >
                        <Button variant="outline" size="sm" onClick={() => void chooseWorkspace()}>
                          Choose folder…
                        </Button>
                        {settings?.workspace_is_custom && (
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => void api.settingsResetWorkspace().then(setSettings)}
                          >
                            Use the default
                          </Button>
                        )}
                      </SettingsRow>
                    </SettingsGroup>

                    <SettingsGroup title="Compiling">
                      <SettingsRow
                        title="TeX installation"
                        description={
                          <>
                            <span className="text-foreground">
                              {tex === null
                                ? "Checking…"
                                : tex.found && tex.install
                                  ? `${tex.install.flavor} found`
                                  : (tex.problem ?? "TeX not found")}
                            </span>
                            {tex?.install && <span className="block font-mono break-all">{tex.install.bin_dir}</span>}
                            <span className="mt-1 block">
                              Only needed if Moss cannot find MiKTeX or TeX Live. Pick the folder that contains{" "}
                              <code>latexmk.exe</code>.{settings?.tex_bin_dir && " A folder is set by hand."}
                            </span>
                          </>
                        }
                      >
                        <Button variant="outline" size="sm" onClick={onSetup}>
                          Set up LaTeX…
                        </Button>
                        <Button variant="outline" size="sm" onClick={() => void chooseTexDir(false)}>
                          Choose folder…
                        </Button>
                        {settings?.tex_bin_dir && (
                          <Button variant="ghost" size="sm" onClick={() => void chooseTexDir(true)}>
                            Search automatically
                          </Button>
                        )}
                      </SettingsRow>
                    </SettingsGroup>
                  </>
                )}

                {page === "project" &&
                  (!project || !projectSettings ? (
                    <SettingsGroup title="This project">
                      <SettingsRow title="No project open" description="Open a project to change how it compiles." />
                    </SettingsGroup>
                  ) : (
                    <>
                      <SettingsGroup title={`Compile · ${project.name}`}>
                        <SettingsRow
                          title="Main file"
                          description="The .tex file with \documentclass that becomes the PDF."
                        >
                          <Select
                            value={projectSettings.root_file}
                            onValueChange={(v) => v && void patchProject({ root_file: v })}
                          >
                            <SelectTrigger className="w-56">
                              <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                              {[...new Set([projectSettings.root_file, ...projectSettings.root_candidates])].map(
                                (f) => (
                                  <SelectItem key={f} value={f}>
                                    {f}
                                  </SelectItem>
                                ),
                              )}
                            </SelectContent>
                          </Select>
                        </SettingsRow>
                        <SettingsRow
                          title="Engine"
                          description="Automatic uses pdfLaTeX, or XeLaTeX when the document needs it."
                        >
                          <Select
                            value={projectSettings.engine}
                            onValueChange={(v) => v && void patchProject({ engine: v as Engine })}
                          >
                            <SelectTrigger className="w-56">
                              <SelectValue>
                                {(v: string) => ENGINES.find((e) => e.value === v)?.label ?? v}
                              </SelectValue>
                            </SelectTrigger>
                            <SelectContent>
                              {ENGINES.map((e) => (
                                <SelectItem key={e.value} value={e.value}>
                                  {e.label}
                                </SelectItem>
                              ))}
                            </SelectContent>
                          </Select>
                        </SettingsRow>
                      </SettingsGroup>

                      <SettingsGroup title="Safety">
                        <SettingsRow
                          title="Allow shell-escape"
                          description="Lets the document run programs while compiling. Needed by minted and a few other packages. Only turn on for projects you trust."
                        >
                          <Switch
                            checked={projectSettings.shell_escape}
                            onCheckedChange={(on) => void patchProject({ shell_escape: on })}
                            aria-label="Allow shell-escape"
                          />
                        </SettingsRow>
                        <SettingsRow
                          title="Trusted project"
                          description="AI agents can delete files and change settings here without asking you first. Every change is still saved to the history, so you can undo it."
                        >
                          <Switch
                            checked={projectSettings.trusted}
                            onCheckedChange={(on) => void patchProject({ trusted: on })}
                            aria-label="Trusted project"
                          />
                        </SettingsRow>
                      </SettingsGroup>
                    </>
                  ))}

                {page === "mcp" && <McpSettings />}

                {page === "updates" && <UpdatesSettings updater={updater} />}
              </div>
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
