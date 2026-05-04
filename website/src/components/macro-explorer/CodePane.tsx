import { useState } from "react"
import { Check, Copy } from "lucide-react"
import { Highlight, themes } from "prism-react-renderer"

import { Button } from "@/components/ui/button"

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
      <Highlight code={code} language="rust" theme={themes.gruvboxMaterialDark}>
        {({ className, getLineProps, getTokenProps, style, tokens }) => (
          <pre
            aria-label={`${label} Rust source code`}
            className={`${className} h-117.5 max-w-full overflow-auto p-4 font-mono text-[12px] leading-6 sm:p-6 sm:text-[13px]`}
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
                    className="mr-4 inline-block w-5 text-right text-stone-700 select-none"
                  >
                    {lineIndex + 1}
                  </span>
                  {line.map((token, tokenIndex) => (
                    <span
                      {...getTokenProps({ token })}
                      key={tokenIndex}
                    />
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
