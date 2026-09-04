import type { PrismTheme } from "prism-react-renderer"

// CSS variables keep every code example in sync with the shared live theme.
export const codeTheme: PrismTheme = {
  plain: { color: "var(--foreground)", backgroundColor: "transparent" },
  styles: [
    {
      types: ["comment", "prolog", "doctype", "cdata"],
      style: { color: "var(--muted-foreground)" },
    },
    {
      types: ["keyword", "operator", "tag"],
      style: { color: "var(--primary)" },
    },
    {
      types: ["string", "char", "attr-value"],
      style: { color: "var(--success)" },
    },
    { types: ["function", "attr-name"], style: { color: "var(--link)" } },
    {
      types: ["number", "boolean", "constant", "class-name", "builtin"],
      style: { color: "var(--integration-event)" },
    },
    { types: ["punctuation"], style: { color: "var(--muted-foreground)" } },
  ],
}
