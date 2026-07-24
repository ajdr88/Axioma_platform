/**
 * "Structural Clarity" brand palette (docs/Axioma_implementation_v3.md §6.2), measured directly
 * from the adopted logomark. These values are the source of truth for the Tailwind `@theme`
 * tokens defined in apps/web's globals.css — keep the two in sync by hand until this package
 * exports a generated CSS file instead.
 *
 * Brand-identity scope note: the two-color Cobalt/Graphite discipline applies to brand/identity
 * surfaces (logo, nav, marketing) only. `alert` is deliberately off-palette (§6.3) so "validated"
 * and "Suspect" are never confusable in the product UI's functional status layer.
 */
export const colors = {
  obsidian: "#07070C",
  graphite: "#7C7C86",
  cobalt: "#052583",
  cobaltGlow: "#3A5BFF",
  paper: "#F3F4F8",
  alert: "#FF5C5C",
} as const;

export type ColorToken = keyof typeof colors;

export const fonts = {
  sans: "'Space Grotesk', ui-sans-serif, system-ui, sans-serif",
  mono: "'JetBrains Mono', ui-monospace, monospace",
} as const;
