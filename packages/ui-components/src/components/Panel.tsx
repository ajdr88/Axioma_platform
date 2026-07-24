import type { HTMLAttributes } from "react";

export type PanelProps = HTMLAttributes<HTMLDivElement>;

/** Glassmorphic panel (impl §6.2: "glassmorphic panels, floating action dock"). */
export function Panel({ className, ...props }: PanelProps) {
  const classes = [
    "rounded-xl border border-white/10 bg-obsidian/70 backdrop-blur-md shadow-2xl",
    className,
  ]
    .filter(Boolean)
    .join(" ");
  return <div className={classes} {...props} />;
}
