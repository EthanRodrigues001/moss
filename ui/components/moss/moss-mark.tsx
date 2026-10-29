import { cn } from "@/lib/utils";

/** The Moss logo: `assets/brand/moss-logo.svg`, copied to `public/moss-logo.svg`. */
export function MossMark({ className }: { className?: string }) {
  return (
    // eslint-disable-next-line @next/next/no-img-element
    <img
      src="/moss-logo.svg"
      alt=""
      aria-hidden
      draggable={false}
      className={cn("size-7 shrink-0 select-none", className)}
    />
  );
}
