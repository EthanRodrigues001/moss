"use client";

import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

/** Hover label for an icon button; `children` is the button element itself. */
export function Tip({
  label,
  side = "top",
  children,
}: {
  label: string;
  side?: "top" | "right" | "bottom";
  children: React.ReactElement;
}) {
  return (
    <Tooltip>
      <TooltipTrigger delay={400} render={children} />
      <TooltipContent side={side}>{label}</TooltipContent>
    </Tooltip>
  );
}
