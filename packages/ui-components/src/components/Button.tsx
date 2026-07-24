import type { ButtonHTMLAttributes } from "react";

type ButtonVariant = "primary" | "ghost";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
}

const base =
  "inline-flex items-center gap-2 rounded-lg px-3 py-1.5 text-sm font-medium transition-colors " +
  "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-cobalt-glow disabled:opacity-40";

const variants: Record<ButtonVariant, string> = {
  primary: "bg-cobalt-glow/90 text-white hover:bg-cobalt-glow",
  ghost: "bg-white/5 text-white/80 border border-white/10 hover:bg-white/10",
};

export function Button({ variant = "primary", className, ...props }: ButtonProps) {
  const classes = [base, variants[variant], className].filter(Boolean).join(" ");
  return <button className={classes} {...props} />;
}
