import { cn } from "@/lib/utils";

/** A labelled card of rows, the way the Settings pages are laid out. */
export function SettingsGroup({
  title,
  aside,
  children,
}: {
  title: string;
  /** Small text or a button on the right of the title. */
  aside?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-3 px-1">
        <h3 className="text-sm font-medium text-muted-foreground">{title}</h3>
        {aside}
      </div>
      <div className="flex flex-col divide-y overflow-hidden rounded-2xl border bg-card/40">{children}</div>
    </section>
  );
}

/** One setting: a title and description on the left, its control on the right. */
export function SettingsRow({
  title,
  description,
  children,
  className,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  children?: React.ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex items-center justify-between gap-6 px-4 py-3", className)}>
      <div className="flex min-w-0 flex-col gap-0.5">
        <div className="text-sm font-medium">{title}</div>
        {description && <div className="text-xs text-muted-foreground">{description}</div>}
      </div>
      {children && <div className="flex shrink-0 items-center gap-2">{children}</div>}
    </div>
  );
}
