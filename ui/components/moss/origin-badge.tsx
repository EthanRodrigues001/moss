import { Cloud, HardDrive } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import type { Origin } from "@/lib/types";

/** The Local / Overleaf tag every Project carries: plain coloured text and icon, no box. */
export function OriginBadge({ origin, className }: { origin: Origin; className?: string }) {
  const overleaf = origin === "overleaf";
  const Icon = overleaf ? Cloud : HardDrive;
  return (
    <Badge
      variant="ghost"
      className={cn(
        "gap-1.5 bg-transparent px-0 font-medium hover:bg-transparent dark:hover:bg-transparent [&>svg]:size-4!",
        overleaf ? "text-emerald-400" : "text-muted-foreground",
        className,
      )}
    >
      <Icon data-icon="inline-start" />
      {overleaf ? "Overleaf" : "Local"}
    </Badge>
  );
}
