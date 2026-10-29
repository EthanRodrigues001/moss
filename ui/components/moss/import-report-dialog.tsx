"use client";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { ImportReport } from "@/lib/types";

type Props = {
  report: ImportReport | null;
  onClose: () => void;
};

/** Shown after an Import when some files could not be brought in (the Needs attention list). */
export function ImportReportDialog({ report, onClose }: Props) {
  return (
    <Dialog open={report !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-lg">
        {report && (
          <>
            <DialogHeader>
              <DialogTitle>Imported “{report.project.name}”</DialogTitle>
              <DialogDescription>
                {report.files_imported} files imported, {report.ignored} generated files left out.
                Root file: <code>{report.project.root_file}</code>.
              </DialogDescription>
            </DialogHeader>

            {report.warnings.map((w) => (
              <p key={w} className="text-sm text-warning">
                {w}
              </p>
            ))}

            {report.skipped.length > 0 && (
              <div className="flex flex-col gap-2">
                <h3 className="text-sm font-medium">
                  Needs attention ({report.skipped.length})
                </h3>
                <p className="text-xs text-muted-foreground">
                  These files were skipped because they cannot be stored safely on Windows.
                  Everything else was imported.
                </p>
                <ScrollArea className="max-h-48 rounded-md border">
                  <ul className="divide-y text-sm">
                    {report.skipped.map((s) => (
                      <li key={s.path} className="px-3 py-2">
                        <div className="font-mono text-xs">{s.path}</div>
                        <div className="text-xs text-muted-foreground">{s.reason}</div>
                      </li>
                    ))}
                  </ul>
                </ScrollArea>
              </div>
            )}

            <DialogFooter>
              <Button onClick={onClose}>Open project</Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
