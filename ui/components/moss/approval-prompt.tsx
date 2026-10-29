"use client";

import { useEffect, useState } from "react";
import { ShieldAlert } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { api } from "@/lib/moss-api";
import type { Approval } from "@/lib/types";

const POLL_MS = 1500;

/** Asks the user before an Agent runs a risky action. Declining (or ignoring it for five minutes) denies it. */
export function ApprovalPrompt({ onAnswered }: { onAnswered?: () => void }) {
  const [pending, setPending] = useState<Approval[]>([]);
  const [answering, setAnswering] = useState(false);

  useEffect(() => {
    let alive = true;
    const poll = () =>
      api
        .approvalsPending()
        .then((list) => alive && setPending(list))
        .catch(() => {});
    void poll();
    const timer = setInterval(poll, POLL_MS);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  const current = pending[0] ?? null;

  async function answer(approve: boolean) {
    if (!current) return;
    setAnswering(true);
    try {
      await api.approvalsRespond(current.id, approve);
      setPending((list) => list.filter((a) => a.id !== current.id));
      onAnswered?.();
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    } finally {
      setAnswering(false);
    }
  }

  return (
    <Dialog open={current !== null} onOpenChange={() => {}}>
      <DialogContent className="sm:max-w-md" showCloseButton={false}>
        {current && (
          <>
            <DialogHeader>
              <DialogTitle className="flex items-center gap-2">
                <ShieldAlert className="size-4 text-warning" />
                An agent wants your approval
              </DialogTitle>
              <DialogDescription>
                In <b>{current.project}</b>. You can undo it afterwards from the history.
              </DialogDescription>
            </DialogHeader>
            <div className="rounded-lg border bg-muted px-3 py-2.5">
              <div className="font-mono text-xs text-muted-foreground">{current.action}</div>
              <div className="mt-1 text-sm">{current.detail}</div>
            </div>
            {pending.length > 1 && (
              <p className="text-xs text-muted-foreground">
                {pending.length - 1} more waiting after this one.
              </p>
            )}
            <DialogFooter>
              <Button variant="outline" disabled={answering} onClick={() => void answer(false)}>
                Deny
              </Button>
              <Button disabled={answering} onClick={() => void answer(true)}>
                Approve
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
