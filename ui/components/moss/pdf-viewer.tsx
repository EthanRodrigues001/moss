"use client";

import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist";
import "pdfjs-dist/web/pdf_viewer.css"; // the text layer's positioning and selection colours
import {
  ArrowLeft,
  ChevronDown,
  ChevronRight,
  Crosshair,
  ExternalLink,
  ListTree,
  Maximize2,
  Save,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Tip } from "./tip";
import { api } from "@/lib/moss-api";
import { cn } from "@/lib/utils";
import type { ForwardHit } from "@/lib/types";

const PAGE_GAP = 12;
const PAGE_PADDING = 12; // the `py-3` around the pages
const MIN_ZOOM = 0.4;
const MAX_ZOOM = 4;
const FLASH_MS = 2500;

type PageSize = { width: number; height: number };

/**
 * Where a source line landed. `hits` are SyncTeX's boxes, one per line of text; `fraction` says how
 * far through the source line the caret is (0 start, 1 end) so a long paragraph lands on the right
 * line of text. `quiet` marks the cursor position while typing: scroll only if it is off-screen,
 * and no flash.
 */
export type PdfHighlight = {
  token: number;
  hits: ForwardHit[];
  fraction: number;
  quiet?: boolean;
};

type Marker = { hit: ForwardHit; quiet: boolean };

/** A clickable box on a page, in PDF points from the page's top-left. */
type PageLink = {
  x: number;
  y: number;
  width: number;
  height: number;
  /** A web or mail address, or where inside this PDF it goes (a name or an explicit destination). */
  url?: string;
  dest?: string | unknown[] | null;
  /** The border the PDF itself asks for (hyperref draws red, green or cyan boxes), as `rgb(...)`. */
  border?: string;
  /** Border thickness in PDF points; 0 means the PDF wants no box. */
  borderWidth: number;
};

const EXTERNAL = /^(https?:|mailto:)/i;

type OutlineNode = {
  title: string;
  dest: string | unknown[] | null;
  url: string | null;
  items: OutlineNode[];
};

const HEADER_ZONE = 0.03;
const FOOTER_ZONE = 0.92;

/**
 * Choose the box to show. Boxes in the page's top or bottom margin are the running header and page
 * number, which SyncTeX also reports for lines like `\newpage`; they are skipped unless that is
 * all there is and the jump was asked for explicitly.
 */
function pickHit(h: PdfHighlight, sizes: PageSize[]): ForwardHit | null {
  const inBody = (hit: ForwardHit) => {
    const page = sizes[hit.page - 1];
    if (!page) return false;
    const middle = hit.y + hit.height / 2;
    return middle > page.height * HEADER_ZONE && middle < page.height * FOOTER_ZONE;
  };
  const order = (a: ForwardHit, b: ForwardHit) => a.page - b.page || a.y - b.y;
  const body = h.hits.filter(inBody).sort(order);
  const pool = body.length > 0 ? body : h.quiet ? [] : [...h.hits].sort(order);
  if (pool.length === 0) return null;
  return pool[Math.round(Math.min(1, Math.max(0, h.fraction)) * (pool.length - 1))];
}

type Props = {
  url: string;
  /** Scroll to this spot and flash it. A new `token` repeats the jump. */
  highlight: PdfHighlight | null;
  /** Double-click: page number and the point in PDF points from the page's top-left. */
  onInverse: (page: number, x: number, y: number) => void;
  /** When given, the toolbar shows a "Follow cursor" switch. */
  follow?: boolean;
  onFollowChange?: (follow: boolean) => void;
  /** The Project folder name. When given, the toolbar offers Save PDF and Open PDF. */
  project?: string | null;
};

/** PDF.js viewer. Pages render lazily; the scroll position survives a recompile. */
export function PdfViewer({ url, highlight, onInverse, follow, onFollowChange, project }: Props) {
  const scroller = useRef<HTMLDivElement>(null);
  const savedScroll = useRef(0);
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [sizes, setSizes] = useState<PageSize[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [width, setWidth] = useState(0);
  const [zoom, setZoom] = useState<number | "fit">("fit");
  const [flash, setFlash] = useState<Marker | null>(null);

  // Load the document whenever the PDF changes.
  useEffect(() => {
    let cancelled = false;
    let loaded: PDFDocumentProxy | null = null;
    (async () => {
      try {
        const pdfjs = await import("pdfjs-dist");
        pdfjs.GlobalWorkerOptions.workerSrc = "/vendor/pdf.worker.min.mjs";
        loaded = await pdfjs.getDocument({ url }).promise;
        const pages = await Promise.all(
          Array.from({ length: loaded.numPages }, async (_, i) => {
            const v = (await loaded!.getPage(i + 1)).getViewport({ scale: 1 });
            return { width: v.width, height: v.height };
          }),
        );
        if (cancelled) {
          void loaded.destroy();
          return;
        }
        setError(null);
        setSizes(pages);
        setDoc(loaded);
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
      if (loaded) void loaded.destroy();
    };
  }, [url]);

  // Put the scroll position back once the new pages have their sizes.
  useLayoutEffect(() => {
    if (scroller.current && sizes.length > 0) scroller.current.scrollTop = savedScroll.current;
  }, [doc, sizes]);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    // One update per frame, and only for a real change: a scrollbar appearing and disappearing
    // as the fitted scale changes must not feed back into the width forever.
    let frame = 0;
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const next = el.clientWidth;
        setWidth((prev) => (Math.abs(prev - next) < 20 && prev !== 0 ? prev : next));
      });
    });
    observer.observe(el);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, []);

  const widest = Math.max(1, ...sizes.map((s) => s.width));
  const scale = zoom === "fit" ? Math.max(0.2, (width - 32) / widest) : zoom;

  // Forward search: scroll to the spot and flash it.
  useEffect(() => {
    if (!highlight || !scroller.current || sizes.length === 0) return;
    const hit = pickHit(highlight, sizes);
    if (!hit) return;
    const marker: Marker = { hit, quiet: Boolean(highlight.quiet) };
    const el = scroller.current;
    let top = PAGE_PADDING;
    for (let i = 0; i < hit.page - 1; i += 1) top += sizes[i].height * scale + PAGE_GAP;
    const target = top + hit.y * scale;
    const margin = el.clientHeight * 0.15;
    const onScreen = target > el.scrollTop + margin && target < el.scrollTop + el.clientHeight - margin;
    if (!highlight.quiet || !onScreen) {
      el.scrollTo({ top: Math.max(0, target - el.clientHeight / 3), behavior: "smooth" });
    }
    const show = setTimeout(() => setFlash(marker), 0);
    if (highlight.quiet) return () => clearTimeout(show); // the cursor marker stays until it moves
    const hide = setTimeout(() => setFlash(null), FLASH_MS);
    return () => {
      clearTimeout(show);
      clearTimeout(hide);
    };
    // Re-run only for a new jump, not when the zoom changes under it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [highlight, sizes]);

  const zoomBy = (factor: number) =>
    setZoom(Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, scale * factor)));

  // Scroll positions to return to after an internal link jump (back from a bibliography entry).
  const [backStack, setBackStack] = useState<number[]>([]);

  const goBack = useCallback(() => {
    setBackStack((stack) => {
      const top = stack[stack.length - 1];
      if (top === undefined) return stack;
      scroller.current?.scrollTo({ top, behavior: "smooth" });
      return stack.slice(0, -1);
    });
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.altKey && e.key === "ArrowLeft" && scroller.current?.contains(document.activeElement)) {
        e.preventDefault();
        goBack();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [goBack]);

  const pageTop = useCallback(
    (index: number) => {
      let top = PAGE_PADDING;
      for (let i = 0; i < index; i += 1) top += (sizes[i]?.height ?? 0) * scale + PAGE_GAP;
      return top;
    },
    [sizes, scale],
  );

  // Page indicator and the "go to page" box.
  const [current, setCurrent] = useState(1);
  const [pageDraft, setPageDraft] = useState<string | null>(null);
  const updateCurrent = useCallback(() => {
    const el = scroller.current;
    if (!el || sizes.length === 0) return;
    const probe = el.scrollTop + el.clientHeight / 3;
    let page = 1;
    for (let i = 0; i < sizes.length; i += 1) if (pageTop(i) <= probe) page = i + 1;
    setCurrent(page);
  }, [sizes, pageTop]);
  useEffect(updateCurrent, [updateCurrent]);

  const goToPage = (n: number) => {
    const page = Math.min(sizes.length, Math.max(1, Math.round(n)));
    scroller.current?.scrollTo({ top: Math.max(0, pageTop(page - 1) - 8), behavior: "smooth" });
  };

  // Outline sidebar: the PDF's own contents.
  const [outline, setOutline] = useState<OutlineNode[]>([]);
  const [showOutline, setShowOutline] = useState(false);
  useEffect(() => {
    if (!doc) return;
    let cancelled = false;
    void doc
      .getOutline()
      .then((o) => !cancelled && setOutline((o ?? []) as OutlineNode[]))
      .catch(() => !cancelled && setOutline([]));
    return () => {
      cancelled = true;
    };
  }, [doc]);

  const savePdf = async () => {
    if (!project) return;
    try {
      if (await api.pdfSaveAs(project)) toast.success("PDF saved");
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  };
  const openPdf = async () => {
    if (!project) return;
    try {
      await api.pdfOpen(project);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  };

  const openLink = useCallback(
    async (link: Pick<PageLink, "url" | "dest">) => {
      if (link.url) {
        if (EXTERNAL.test(link.url)) void api.openExternal(link.url).catch(() => {});
        return;
      }
      const el = scroller.current;
      if (!doc || !el || !link.dest) return;
      try {
        const dest = typeof link.dest === "string" ? await doc.getDestination(link.dest) : link.dest;
        if (!Array.isArray(dest)) return;
        const ref = dest[0];
        const index = typeof ref === "number" ? ref : await doc.getPageIndex(ref as never);
        const size = sizes[index];
        if (!size) return;
        // XYZ and FitH style destinations carry the target's height up from the page bottom.
        const kind = (dest[1] as { name?: string } | null)?.name;
        const fromBottom = kind === "XYZ" ? dest[3] : kind === "FitH" || kind === "FitBH" ? dest[2] : null;
        const y = typeof fromBottom === "number" ? Math.max(0, size.height - fromBottom) : 0;
        let top = PAGE_PADDING;
        for (let i = 0; i < index; i += 1) top += sizes[i].height * scale + PAGE_GAP;
        setBackStack((stack) => [...stack.slice(-49), el.scrollTop]);
        el.scrollTo({ top: Math.max(0, top + y * scale - 24), behavior: "smooth" });
      } catch {
        // A broken destination in the PDF: stay where we are.
      }
    },
    [doc, sizes, scale],
  );

  return (
    <div
      className="flex h-full min-h-0 flex-col outline-none"
      tabIndex={-1}
      onPointerDownCapture={(e) => {
        if (!(e.target instanceof HTMLInputElement)) e.currentTarget.focus({ preventScroll: true });
      }}
    >
      <div className="flex h-8 shrink-0 items-center gap-1 border-b px-2 text-xs text-muted-foreground">
        {outline.length > 0 && (
          <Tip label="Contents" side="bottom">
            <Button
              variant={showOutline ? "secondary" : "ghost"}
              size="icon-xs"
              aria-label="Outline"
              aria-pressed={showOutline}
              onClick={() => setShowOutline((v) => !v)}
            >
              <ListTree />
            </Button>
          </Tip>
        )}
        {backStack.length > 0 && (
          <Tip label="Back to where you were (Alt+Left)" side="bottom">
            <Button variant="ghost" size="xs" onClick={goBack}>
              <ArrowLeft data-icon="inline-start" />
              Back
            </Button>
          </Tip>
        )}
        {sizes.length > 0 && (
          <span className="flex items-center gap-1 tabular-nums" title="Go to page">
            <input
              aria-label="Page number"
              className="h-5 w-9 rounded border border-input bg-background px-1 text-center outline-none focus-visible:border-ring"
              value={pageDraft ?? String(current)}
              inputMode="numeric"
              onFocus={(e) => e.currentTarget.select()}
              onChange={(e) => setPageDraft(e.target.value.replace(/\D/g, ""))}
              onBlur={() => setPageDraft(null)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && pageDraft) {
                  goToPage(Number(pageDraft));
                  setPageDraft(null);
                  e.currentTarget.blur();
                } else if (e.key === "Escape") {
                  setPageDraft(null);
                  e.currentTarget.blur();
                }
              }}
            />
            / {sizes.length}
          </span>
        )}
        <div className="ml-auto flex items-center gap-0.5">
          {project && (
            <>
              <Tip label="Save PDF as…" side="bottom">
                <Button variant="ghost" size="icon-xs" aria-label="Save PDF as" onClick={() => void savePdf()}>
                  <Save />
                </Button>
              </Tip>
              <Tip label="Open in your PDF app" side="bottom">
                <Button variant="ghost" size="icon-xs" aria-label="Open PDF" onClick={() => void openPdf()}>
                  <ExternalLink />
                </Button>
              </Tip>
            </>
          )}
          {onFollowChange && (
            <Tip label="Keep the PDF at the line your cursor is on" side="bottom">
              <Button
                variant={follow ? "secondary" : "ghost"}
                size="xs"
                aria-pressed={follow}
                onClick={() => onFollowChange(!follow)}
              >
                <Crosshair data-icon="inline-start" />
                Follow cursor
              </Button>
            </Tip>
          )}
          <Tip label="Zoom out" side="bottom">
            <Button variant="ghost" size="icon-xs" aria-label="Zoom out" onClick={() => zoomBy(1 / 1.2)}>
              <ZoomOut />
            </Button>
          </Tip>
          <span className="w-10 text-center tabular-nums">{Math.round(scale * 100)}%</span>
          <Tip label="Zoom in" side="bottom">
            <Button variant="ghost" size="icon-xs" aria-label="Zoom in" onClick={() => zoomBy(1.2)}>
              <ZoomIn />
            </Button>
          </Tip>
          <Tip label="Fit to width" side="bottom">
            <Button variant="ghost" size="icon-xs" aria-label="Fit width" onClick={() => setZoom("fit")}>
              <Maximize2 />
            </Button>
          </Tip>
        </div>
      </div>

      <div className="flex min-h-0 flex-1">
      {showOutline && outline.length > 0 && (
        <nav aria-label="PDF contents" className="w-56 shrink-0 overflow-auto border-r py-1 text-xs">
          <OutlineList nodes={outline} depth={0} onPick={(n) => void openLink({ url: n.url ?? undefined, dest: n.dest })} />
        </nav>
      )}
      <div
        ref={scroller}
        className="min-h-0 min-w-0 flex-1 overflow-auto bg-muted/30 [scrollbar-gutter:stable]"
        onScroll={(e) => {
          savedScroll.current = e.currentTarget.scrollTop;
          updateCurrent();
        }}
      >
        {error ? (
          <p className="p-6 text-center text-sm text-destructive">Could not show the PDF: {error}</p>
        ) : (
          <div className="mx-auto flex w-max min-w-full flex-col items-center py-3" style={{ gap: PAGE_GAP }}>
            {doc &&
              sizes.map((size, i) => (
                <PdfPage
                  key={`${doc.fingerprints[0]}-${i}`}
                  doc={doc}
                  number={i + 1}
                  size={size}
                  scale={scale}
                  flash={flash?.hit.page === i + 1 ? flash : null}
                  onInverse={onInverse}
                  onLink={openLink}
                />
              ))}
          </div>
        )}
      </div>
      </div>
    </div>
  );
}

function OutlineList({
  nodes,
  depth,
  onPick,
}: {
  nodes: OutlineNode[];
  depth: number;
  onPick: (node: OutlineNode) => void;
}) {
  return (
    <ul>
      {nodes.map((n, i) => (
        <OutlineItem key={i} node={n} depth={depth} onPick={onPick} />
      ))}
    </ul>
  );
}

function OutlineItem({
  node,
  depth,
  onPick,
}: {
  node: OutlineNode;
  depth: number;
  onPick: (node: OutlineNode) => void;
}) {
  const [open, setOpen] = useState(depth === 0);
  const hasKids = node.items.length > 0;
  return (
    <li>
      <div className="flex items-start" style={{ paddingLeft: 4 + depth * 12 }}>
        <button
          type="button"
          aria-label={open ? "Collapse" : "Expand"}
          className={cn("mt-0.5 flex size-4 shrink-0 items-center justify-center", !hasKids && "invisible")}
          onClick={() => setOpen((v) => !v)}
        >
          {open ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}
        </button>
        <button
          type="button"
          className="min-w-0 flex-1 rounded px-1 py-0.5 text-left hover:bg-accent"
          title={node.title}
          onClick={() => onPick(node)}
        >
          {node.title}
        </button>
      </div>
      {hasKids && open && <OutlineList nodes={node.items} depth={depth + 1} onPick={onPick} />}
    </li>
  );
}

function PdfPage({
  doc,
  number,
  size,
  scale,
  flash,
  onInverse,
  onLink,
}: {
  doc: PDFDocumentProxy;
  number: number;
  size: PageSize;
  scale: number;
  flash: Marker | null;
  onInverse: Props["onInverse"];
  onLink: (link: PageLink) => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const textLayer = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  const [links, setLinks] = useState<PageLink[]>([]);

  // Link boxes: read once per page, kept in PDF points so zooming only rescales them.
  useEffect(() => {
    if (!visible) return;
    let cancelled = false;
    (async () => {
      const page = await doc.getPage(number);
      const viewport = page.getViewport({ scale: 1 });
      const found: PageLink[] = [];
      for (const a of await page.getAnnotations({ intent: "display" })) {
        if (a.subtype !== "Link" || !a.rect) continue;
        const [x1, y1, x2, y2] = viewport.convertToViewportRectangle(a.rect);
        const url: string | undefined = a.url ?? a.unsafeUrl;
        if (!url && !a.dest) continue;
        found.push({
          x: Math.min(x1, x2),
          y: Math.min(y1, y2),
          width: Math.abs(x2 - x1),
          height: Math.abs(y2 - y1),
          url,
          dest: a.dest,
          border: a.color ? `rgb(${a.color[0]}, ${a.color[1]}, ${a.color[2]})` : undefined,
          borderWidth: a.borderStyle?.width ?? 1,
        });
      }
      if (!cancelled) setLinks(found);
    })().catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [visible, doc, number]);

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const observer = new IntersectionObserver(([entry]) => entry.isIntersecting && setVisible(true), {
      rootMargin: "800px 0px",
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!visible) return;
    let task: { cancel: () => void; promise: Promise<unknown> } | null = null;
    let text: { cancel: () => void } | null = null;
    let cancelled = false;
    (async () => {
      const page = await doc.getPage(number);
      const el = canvas.current;
      const ctx = el?.getContext("2d");
      if (cancelled || !el || !ctx) return;
      const ratio = window.devicePixelRatio || 1;
      const viewport = page.getViewport({ scale });
      el.width = Math.floor(viewport.width * ratio);
      el.height = Math.floor(viewport.height * ratio);
      ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
      task = page.render({ canvas: el, canvasContext: ctx, viewport });

      // The invisible text on top of the canvas is what makes text selectable and copyable.
      const container = textLayer.current;
      if (container) {
        const pdfjs = await import("pdfjs-dist");
        if (cancelled) return;
        container.replaceChildren();
        const layer = new pdfjs.TextLayer({
          textContentSource: page.streamTextContent(),
          container,
          viewport,
        });
        text = layer;
        void layer.render().catch(() => {});
      }
      await task.promise.catch(() => {}); // a cancelled render is expected on zoom
    })().catch(() => {
      // A recompile replaces the document and destroys the old one while a page is still being
      // drawn; the effect is cleaned up and re-run for the new document, so the failure is moot.
    });
    return () => {
      cancelled = true;
      task?.cancel();
      text?.cancel();
    };
  }, [visible, doc, number, scale]);

  return (
    <div
      ref={box}
      className="relative bg-white shadow-md"
      style={{ width: size.width * scale, height: size.height * scale }}
      onDoubleClick={(e) => {
        const rect = e.currentTarget.getBoundingClientRect();
        onInverse(number, (e.clientX - rect.left) / scale, (e.clientY - rect.top) / scale);
      }}
    >
      <canvas ref={canvas} className="h-full w-full" />
      <div
        ref={textLayer}
        className="textLayer"
        style={{ "--scale-factor": scale, "--total-scale-factor": scale } as React.CSSProperties}
      />
      {links.map((l, i) => (
        <a
          key={i}
          href={l.url ?? "#"}
          title={l.url}
          className="absolute z-10 box-border cursor-pointer hover:bg-primary/15"
          style={{
            left: l.x * scale,
            top: l.y * scale,
            width: l.width * scale,
            height: l.height * scale,
            // Drawn like Overleaf and other viewers: the box colour and width come from the PDF.
            border: l.borderWidth > 0 && l.border ? `${Math.max(1, l.borderWidth * scale)}px solid ${l.border}` : undefined,
          }}
          onClick={(e) => {
            e.preventDefault();
            onLink(l);
          }}
          onDoubleClick={(e) => e.stopPropagation()}
        />
      ))}
      {flash &&
        (flash.quiet ? (
          <div
            className="pointer-events-none absolute rounded-full bg-primary"
            style={{
              left: flash.hit.x * scale,
              top: (flash.hit.y + flash.hit.height) * scale,
              width: flash.hit.width * scale,
              height: 3,
            }}
          />
        ) : (
          <div
            className="pointer-events-none absolute animate-pulse rounded-sm bg-primary/25 ring-2 ring-primary"
            style={{
              left: flash.hit.x * scale - 2,
              top: flash.hit.y * scale - 2,
              width: flash.hit.width * scale + 4,
              height: flash.hit.height * scale + 4,
            }}
          />
        ))}
    </div>
  );
}
