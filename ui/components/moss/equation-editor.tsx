"use client";

import katex from "katex";
import { useEffect, useRef, useState } from "react";

export function EquationEditor({
  compact = false,
  value,
  onChange,
}: {
  compact?: boolean;
  value: string;
  onChange: (value: string) => void;
}) {
  const fieldRef = useRef<HTMLElement | null>(null);
  const preview = katex.renderToString(value || " ", { displayMode: true, throwOnError: false, strict: false });

  const [ready, setReady] = useState(false);

  // MathLive defines <math-field> when it loads; fonts come from the local copy, not a CDN.
  useEffect(() => {
    let alive = true;
    void import("mathlive").then(({ MathfieldElement }) => {
      MathfieldElement.fontsDirectory = "/vendor/mathlive/fonts";
      if (alive) setReady(true);
    });
    return () => {
      alive = false;
    };
  }, []);

  useEffect(() => {
    const field = fieldRef.current as (HTMLElement & { value?: string }) | null;
    if (ready && field && field.value !== value) field.value = value;
  }, [value, ready]);

  if (compact) {
    return (
      <span className="inline-flex max-w-full flex-col gap-2 rounded-md border bg-background p-2 align-middle">
        <math-field
          ref={(element) => {
            fieldRef.current = element;
          }}
          className="min-h-10 min-w-64 rounded-md border bg-background p-2 text-base"
          onInput={(event) => onChange((event.currentTarget as HTMLElement & { value?: string }).value ?? "")}
        />
        <span className="overflow-auto rounded-md bg-muted/40 p-2 text-center" dangerouslySetInnerHTML={{ __html: preview }} />
      </span>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <math-field
        ref={(element) => {
          fieldRef.current = element;
        }}
        className="min-h-20 rounded-md border bg-background p-2 text-lg"
        onInput={(event) => onChange((event.currentTarget as HTMLElement & { value?: string }).value ?? "")}
      />
      <div className="rounded-md border bg-muted/40 p-4 text-center" dangerouslySetInnerHTML={{ __html: preview }} />
    </div>
  );
}
