import { useState } from "react"
import { Check, Copy } from "lucide-react"
import { Highlight } from "prism-react-renderer"

import { Button } from "@/components/ui/button"
import { codeTheme } from "@/lib/code-theme"

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
      <div className="flex items-center justify-between gap-2 border-b border-border px-3 py-2">
        <span
          className="min-w-0 truncate font-mono text-[10px] text-muted-foreground"
          title={label}
        >
          {label}
        </span>
        <Button
          aria-label={`Copy ${label}`}
          className="shrink-0 text-muted-foreground hover:text-foreground"
          onClick={copyCode}
          size="xs"
          variant="ghost"
        >
          {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
          <span aria-live="polite">{copied ? "Copied" : "Copy"}</span>
        </Button>
      </div>
      <Highlight code={code} language="rust" theme={codeTheme}>
        {({ className, getLineProps, getTokenProps, style, tokens }) => (
          <pre
            aria-label={`${label} Rust source code`}
            className={`${className} h-96 max-w-full overflow-auto p-3 font-mono text-[11px] leading-6`}
            style={{ ...style, backgroundColor: "transparent" }}
            tabIndex={0}
          >
            <code>
              {tokens.map((line, lineIndex) => (
                <span
                  {...getLineProps({ line })}
                  className="block min-w-max"
                  key={lineIndex}
                >
                  <span
                    aria-hidden="true"
                    className="mr-4 inline-block w-5 text-right text-muted-foreground select-none"
                  >
                    {lineIndex + 1}
                  </span>
                  {line.map((token, tokenIndex) => (
                    <span {...getTokenProps({ token })} key={tokenIndex} />
                  ))}
                  {"\n"}
                </span>
              ))}
            </code>
          </pre>
        )}
      </Highlight>
    </div>
  )
}
