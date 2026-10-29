"use client";

import { useEffect, useState } from "react";
import { FileQuestion } from "lucide-react";
import { api } from "@/lib/moss-api";
import type { FileEntry } from "@/lib/types";
import { PdfViewer } from "./pdf-viewer";

const BROWSER_IMAGES = new Set(["png", "jpg", "jpeg", "gif", "svg", "bmp", "webp"]);

const extension = (path: string) => path.split(".").pop()?.toLowerCase() ?? "";

const formatSize = (bytes: number) =>
  bytes < 1024
    ? `${bytes} B`
    : bytes < 1024 * 1024
      ? `${(bytes / 1024).toFixed(1)} KB`
      : `${(bytes / (1024 * 1024)).toFixed(1)} MB`;

type Props = {
  project: string;
  file: FileEntry;
};

/** Preview for files the editor cannot open: images and PDFs, with a note for anything else. */
export function FilePreview({ project, file }: Props) {
  const ext = extension(file.path);
  const previewable = file.kind === "pdf" || (file.kind === "image" && BROWSER_IMAGES.has(ext));
  const [loaded, setLoaded] = useState<{ key: string; url: string | null; error: string | null }>();
  const key = `${project}/${file.path}`;

  useEffect(() => {
    if (!previewable) return;
    let alive = true;
    let objectUrl: string | null = null;
    api
      .fileUrl(project, file.path)
      .then((url) => {
        if (!alive) return URL.revokeObjectURL(url);
        objectUrl = url;
        setLoaded({ key, url, error: null });
      })
      .catch((e) => alive && setLoaded({ key, url: null, error: e instanceof Error ? e.message : String(e) }));
    return () => {
      alive = false;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [previewable, project, file.path, key]);

  // Ignore a result that belongs to the previously selected file.
  const current = loaded?.key === key ? loaded : undefined;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-9 shrink-0 items-center gap-2 border-b px-3">
        <span className="min-w-0 truncate font-mono text-xs text-muted-foreground">{file.path}</span>
        <span className="ml-auto shrink-0 text-xs text-muted-foreground">{formatSize(file.size)}</span>
      </div>

      <div className="min-h-0 flex-1">
        {!previewable ? (
          <Notice>
            No preview for .{ext || "this"} files here. LaTeX can still use it from your document.
          </Notice>
        ) : current?.error ? (
          <Notice>{current.error}</Notice>
        ) : !current?.url ? (
          <Notice>Loading…</Notice>
        ) : file.kind === "pdf" ? (
          <PdfViewer url={current.url} highlight={null} onInverse={() => {}} />
        ) : (
          <div className="flex h-full items-center justify-center overflow-auto bg-muted/30 p-6">
            {/* eslint-disable-next-line @next/next/no-img-element */}
            <img
              src={current.url}
              alt={file.path}
              className="max-h-full max-w-full rounded-md border bg-white object-contain shadow-md"
            />
          </div>
        )}
      </div>
    </div>
  );
}

function Notice({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 px-6 text-center text-sm text-muted-foreground">
      <FileQuestion className="size-7 opacity-50" />
      {children}
    </div>
  );
}
