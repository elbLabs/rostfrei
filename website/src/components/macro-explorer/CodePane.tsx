import { useState, type ReactNode } from "react"
import { Check, Copy } from "lucide-react"

import { Button } from "@/components/ui/button"

const TOKEN_PATTERN =
  /(\/\/.*|#\[[^\]]+\]|"(?:\\.|[^"\\])*"|\b(?:as|const|enum|fn|for|impl|let|pub|struct|trait|type|where|Self|self|mut|return)\b|\b[A-Z][A-Za-z0-9_]*\b)/g

const RUST_KEYWORDS = new Set([
  "as",
  "const",
  "enum",
  "fn",
  "for",
  "impl",
  "let",
  "mut",
  "pub",
  "return",
  "self",
  "Self",
  "struct",
  "trait",
  "type",
  "where",
])

function tokenColor(token: string): string | null {
  if (token.startsWith("//")) return "text-stone-500"
  if (token.startsWith("#[")) return "text-amber-300"
  if (token.startsWith('"')) return "text-orange-300"
  if (/^[A-Z]/.test(token)) return "text-yellow-100"
  if (RUST_KEYWORDS.has(token)) return "text-rose-300"
  return null
}

function highlightedLine(line: string): ReactNode[] {
  return line.split(TOKEN_PATTERN).map((token, index) => {
    const color = tokenColor(token)
    return color ? (
      <span className={color} key={`${token}-${index}`}>
        {token}
      </span>
    ) : (
      token
    )
  })
}

export function CodePane({ code, label }: { code: string; label: string }) {
  const [copied, setCopied] = useState(false)

  async function copyCode() {
    try {
      await navigator.clipboard.writeText(code)
      setCopied(true)
      window.setTimeout(() => setCopied(false), 1600)
    } catch {
      setCopied(false)
    }
  }

  return (
    <div className="min-w-0">
      <div className="flex items-center justify-between border-b border-white/8 px-4 py-2.5">
        <span className="font-mono text-[10px] tracking-[0.12em] text-stone-500 uppercase">
          {label}
        </span>
        <Button
          aria-label={`Copy ${label}`}
          className="text-stone-500 hover:bg-white/5 hover:text-stone-200"
          onClick={copyCode}
          size="xs"
          variant="ghost"
        >
          {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
          <span aria-live="polite">{copied ? "Copied" : "Copy"}</span>
        </Button>
      </div>
      <pre
        className="h-[470px] max-w-full overflow-auto p-4 font-mono text-[12px] leading-6 text-stone-300 sm:p-6 sm:text-[13px]"
        tabIndex={0}
      >
        <code>
          {code.split("\n").map((line, index) => (
            <span className="block min-w-max" key={`${line}-${index}`}>
              <span
                aria-hidden="true"
                className="mr-4 inline-block w-5 text-right text-stone-700 select-none"
              >
                {index + 1}
              </span>
              {highlightedLine(line)}
              {"\n"}
            </span>
          ))}
        </code>
      </pre>
    </div>
  )
}
