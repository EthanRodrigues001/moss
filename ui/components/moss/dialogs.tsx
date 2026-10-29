"use client";

import { createContext, useCallback, useContext, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";

export type PromptOptions = {
  title: string;
  description?: string;
  label?: string;
  initial?: string;
  placeholder?: string;
  /** A multi-line box in a code font, for LaTeX. */
  multiline?: boolean;
  confirmLabel?: string;
};

export type ConfirmOptions = {
  title: string;
  description?: string;
  confirmLabel?: string;
  /** Red confirm button, for deleting things. */
  destructive?: boolean;
  /** The user must type this exactly before the confirm button works (for deleting a project). */
  requireText?: string;
};

type Dialogs = {
  /** Ask for text. Resolves to null when cancelled. */
  prompt: (options: PromptOptions) => Promise<string | null>;
  /** Ask a yes/no question. Resolves to false when cancelled. */
  confirm: (options: ConfirmOptions) => Promise<boolean>;
};

type Pending =
  | { id: number; kind: "prompt"; options: PromptOptions; resolve: (v: string | null) => void }
  | { id: number; kind: "confirm"; options: ConfirmOptions; resolve: (v: boolean) => void };

const DialogsContext = createContext<Dialogs | null>(null);

/** Moss's own replacements for `window.prompt` and `window.confirm`. */
export function useDialogs(): Dialogs {
  const dialogs = useContext(DialogsContext);
  if (!dialogs) throw new Error("useDialogs needs a <DialogsProvider>");
  return dialogs;
}

export function DialogsProvider({ children }: { children: React.ReactNode }) {
  const [pending, setPending] = useState<Pending | null>(null);
  const nextId = useRef(0);

  const prompt = useCallback(
    (options: PromptOptions) =>
      new Promise<string | null>((resolve) => {
        nextId.current += 1;
        setPending({ id: nextId.current, kind: "prompt", options, resolve });
      }),
    [],
  );
  const confirm = useCallback(
    (options: ConfirmOptions) =>
      new Promise<boolean>((resolve) => {
        nextId.current += 1;
        setPending({ id: nextId.current, kind: "confirm", options, resolve });
      }),
    [],
  );
  const value = useMemo(() => ({ prompt, confirm }), [prompt, confirm]);

  function close() {
    setPending(null);
  }

  return (
    <DialogsContext.Provider value={value}>
      {children}
      <Dialog open={pending !== null} onOpenChange={(open) => !open && cancel()}>
        <DialogContent className="sm:max-w-md" showCloseButton={false}>
          {pending?.kind === "prompt" && (
            <PromptBody
              key={pending.id}
              options={pending.options}
              onSubmit={(text) => {
                pending.resolve(text);
                close();
              }}
              onCancel={cancel}
            />
          )}
          {pending?.kind === "confirm" && (
            <ConfirmBody
              key={pending.id}
              options={pending.options}
              onAnswer={(yes) => {
                pending.resolve(yes);
                close();
              }}
            />
          )}
        </DialogContent>
      </Dialog>
    </DialogsContext.Provider>
  );

  function cancel() {
    if (pending?.kind === "prompt") pending.resolve(null);
    if (pending?.kind === "confirm") pending.resolve(false);
    close();
  }
}

function PromptBody({
  options,
  onSubmit,
  onCancel,
}: {
  options: PromptOptions;
  onSubmit: (text: string) => void;
  onCancel: () => void;
}) {
  const [text, setText] = useState(options.initial ?? "");
  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        onSubmit(text);
      }}
    >
      <DialogHeader>
        <DialogTitle>{options.title}</DialogTitle>
        {options.description && <DialogDescription>{options.description}</DialogDescription>}
      </DialogHeader>
      <label className="flex flex-col gap-1.5 text-sm">
        {options.label && <span className="font-medium">{options.label}</span>}
        {options.multiline ? (
          <textarea
            autoFocus
            value={text}
            spellCheck={false}
            placeholder={options.placeholder}
            onChange={(e) => setText(e.target.value)}
            onKeyDown={(e) => {
              if ((e.ctrlKey || e.metaKey) && e.key === "Enter") onSubmit(text);
            }}
            className="min-h-48 w-full resize-y rounded-lg border bg-input/30 p-3 font-mono text-xs outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/40"
          />
        ) : (
          <Input
            autoFocus
            value={text}
            placeholder={options.placeholder}
            onChange={(e) => setText(e.target.value)}
            onFocus={(e) => e.currentTarget.select()}
          />
        )}
      </label>
      <DialogFooter>
        <Button type="button" variant="outline" onClick={onCancel}>
          Cancel
        </Button>
        <Button type="submit">{options.confirmLabel ?? "OK"}</Button>
      </DialogFooter>
    </form>
  );
}

function ConfirmBody({
  options,
  onAnswer,
}: {
  options: ConfirmOptions;
  onAnswer: (yes: boolean) => void;
}) {
  const [typed, setTyped] = useState("");
  const locked = options.requireText !== undefined && typed.trim() !== options.requireText;
  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        if (!locked) onAnswer(true);
      }}
    >
      <DialogHeader>
        <DialogTitle>{options.title}</DialogTitle>
        {options.description && <DialogDescription>{options.description}</DialogDescription>}
      </DialogHeader>
      {options.requireText !== undefined && (
        <label className="flex flex-col gap-1.5 text-sm">
          <span className="text-muted-foreground">
            Type <span className="font-mono font-medium text-foreground">{options.requireText}</span> to confirm
          </span>
          <Input autoFocus value={typed} onChange={(e) => setTyped(e.target.value)} placeholder={options.requireText} />
        </label>
      )}
      <DialogFooter>
        <Button type="button" variant="outline" onClick={() => onAnswer(false)}>
          Cancel
        </Button>
        <Button
          type="submit"
          autoFocus={options.requireText === undefined}
          disabled={locked}
          variant={options.destructive ? "destructive" : "default"}
        >
          {options.confirmLabel ?? "OK"}
        </Button>
      </DialogFooter>
    </form>
  );
}
