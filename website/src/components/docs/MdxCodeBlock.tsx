import {
  Children,
  isValidElement,
  useEffect,
  useRef,
  useState,
  type ComponentPropsWithoutRef,
  type ReactNode,
} from "react"
import { Check, Copy } from "lucide-react"
import { Highlight, themes, type Language } from "prism-react-renderer"

import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

const languageAliases: Record<string, Language> = {
  html: "markup",
  js: "javascript",
  md: "markdown",
  py: "python",
  rs: "rust",
  ts: "typescript",
  yml: "yaml",
}

const languageLabels: Record<string, string> = {
  bash: "Shell",
  css: "CSS",
  go: "Go",
  html: "HTML",
  javascript: "JavaScript",
  json: "JSON",
  jsx: "JSX",
  markdown: "Markdown",
  plaintext: "Text",
  python: "Python",
  rust: "Rust",
  shell: "Shell",
  sql: "SQL",
  toml: "TOML",
  tsx: "TSX",
  typescript: "TypeScript",
  yaml: "YAML",
}

interface CodeElementProps {
  children?: ReactNode
  className?: string
}

function codeText(children: ReactNode): string {
  return Children.toArray(children)
    .map((child) => (typeof child === "string" ? child : String(child)))
    .join("")
    .replace(/\n$/, "")
}

function codeDetails(children: ReactNode) {
  if (!isValidElement<CodeElementProps>(children)) {
    return { code: codeText(children), language: "plaintext" }
  }

  const languageMatch = children.props.className?.match(/language-([\w-]+)/)
  const requestedLanguage = languageMatch?.[1]?.toLowerCase() ?? "plaintext"

  return {
    code: codeText(children.props.children),
    language: languageAliases[requestedLanguage] ?? requestedLanguage,
  }
}

export function MdxCodeBlock({
  children,
  className,
  ...props
}: ComponentPropsWithoutRef<"pre">) {
  const [copied, setCopied] = useState(false)
  const resetTimer = useRef<number | undefined>(undefined)
  const { code, language } = codeDetails(children)
  const label = languageLabels[language] ?? language.toUpperCase()

  useEffect(
    () => () => {
      if (resetTimer.current !== undefined) {
        window.clearTimeout(resetTimer.current)
      }
    },
    []
  )

  async function copyCode() {
    try {
      await navigator.clipboard.writeText(code)
      setCopied(true)
      window.clearTimeout(resetTimer.current)
      resetTimer.current = window.setTimeout(() => setCopied(false), 1600)
    } catch {
      setCopied(false)
    }
  }

  return (
    <div
      className="my-6 overflow-hidden rounded-xl border border-border bg-[#14110d]"
      data-slot="mdx-code-block"
    >
      <div className="flex items-center justify-between border-b border-border/70 px-4 py-2">
        <span className="font-mono text-[10px] tracking-[0.12em] text-muted-foreground uppercase">
          {label}
        </span>
        <Button
          aria-label="Copy code"
          className="text-muted-foreground hover:text-foreground"
          onClick={copyCode}
          size="xs"
          type="button"
          variant="ghost"
        >
          {copied ? <Check aria-hidden="true" /> : <Copy aria-hidden="true" />}
          <span aria-live="polite">{copied ? "Copied" : "Copy"}</span>
        </Button>
      </div>
      <Highlight
        code={code}
        language={language}
        theme={themes.gruvboxMaterialDark}
      >
        {({
          className: prismClassName,
          getLineProps,
          getTokenProps,
          style,
          tokens,
        }) => (
          <pre
            {...props}
            aria-label={`${label} code example`}
            className={cn(
              prismClassName,
              "max-w-full overflow-auto p-4 font-mono text-[12px] leading-6 sm:px-5 sm:text-[13px]",
              className
            )}
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
                  {line.map((token, tokenIndex) => (
                    <span {...getTokenProps({ token })} key={tokenIndex} />
                  ))}
                  {lineIndex < tokens.length - 1 ? "\n" : null}
                </span>
              ))}
            </code>
          </pre>
        )}
      </Highlight>
    </div>
  )
}
