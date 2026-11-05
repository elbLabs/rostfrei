import type { PrismTheme } from "prism-react-renderer"

// Syntax colors have their own reading palette in each website theme.
export const codeTheme: PrismTheme = {
  plain: { color: "var(--code-foreground)", backgroundColor: "transparent" },
  styles: [
    {
      types: ["comment", "prolog", "doctype", "cdata"],
      style: { color: "var(--code-comment)" },
    },
    {
      types: ["keyword", "tag"],
      style: { color: "var(--code-keyword)" },
    },
    {
      types: ["string", "char", "attr-value"],
      style: { color: "var(--code-string)" },
    },
    {
      types: ["function", "attr-name"],
      style: { color: "var(--code-function)" },
    },
    {
      types: ["class-name", "builtin"],
      style: { color: "var(--code-type)" },
    },
    {
      types: ["number", "boolean", "constant"],
      style: { color: "var(--code-number)" },
    },
    {
      types: ["punctuation", "operator"],
      style: { color: "var(--code-punctuation)" },
    },
  ],
}
