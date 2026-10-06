import { existsSync, readFileSync, statSync } from "node:fs"
import { dirname, relative, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import type { Plugin } from "vite"

import { docsPages } from "./src/docs/navigation.ts"

const repository = fileURLToPath(new URL("../", import.meta.url))
const docs = resolve(repository, "docs")
const routes = new Map(
  docsPages.map((page) => [resolve(docs, page.file), page.slug])
)

// Source links work in a checkout; the two published views keep their own URLs.
export function documentationHref(
  href: string,
  document: string,
  view: "website" | "source" = "website"
): string {
  if (/^(?:[a-z][a-z\d+.-]*:|\/|#)/i.test(href)) return href
  const [, path, suffix = ""] = /^([^?#]*)(.*)$/.exec(href) ?? []
  if (!path) return href
  const target = resolve(dirname(document), decodeURIComponent(path))
  if (!existsSync(target)) throw new Error(`Broken documentation link in ${document}: ${href}`)
  const slug = routes.get(target)
  if (slug !== undefined) {
    return view === "source" ? href : `/docs${slug ? `/${slug}` : ""}${suffix}`
  }
  const sourcePath = relative(repository, target).replaceAll("\\", "/")
  if (sourcePath.startsWith("../")) throw new Error(`Link outside repository: ${href}`)
  const kind = statSync(target).isDirectory() ? "tree" : "blob"
  return `https://github.com/elbLabs/rostfrei/${kind}/main/${sourcePath}${suffix}`
}

interface MarkdownNode {
  type: string
  url?: string
  children?: MarkdownNode[]
}

export function remarkDocumentationLinks() {
  return (tree: MarkdownNode, file: { path: string }) => {
    function visit(node: MarkdownNode) {
      if ((node.type === "link" || node.type === "definition") && node.url) {
        node.url = documentationHref(node.url, file.path)
      }
      node.children?.forEach(visit)
    }
    visit(tree)
  }
}

export function documentationSource(source: string, document: string): string {
  // Preserve code verbatim, including Markdown examples inside fenced blocks.
  let fence: string | undefined
  return source.split("\n").map((line) => {
    const marker = /^\s*(`{3,}|~{3,})/.exec(line)?.[1]
    if (marker) {
      if (!fence) fence = marker
      else if (marker[0] === fence[0] && marker.length >= fence.length) fence = undefined
      return line
    }
    if (fence) return line
    return line
      .replace(/\]\(([^\s)]+)\)/g, (_, href: string) =>
        `](${documentationHref(href, document, "source")})`)
      .replace(/^(\s*\[[^\]]+\]:\s*)(\S+)/, (_, prefix: string, href: string) =>
        prefix + documentationHref(href, document, "source"))
  }).join("\n")
}

export function documentationPlugin(): Plugin {
  let base = "/"
  return {
    name: "rostfrei-shared-documentation",
    configResolved(config) { base = config.base },
    generateBundle() {
      const index = [
        "# Rostfrei documentation",
        "",
        "> Official documentation shared by developers and coding agents.",
        "> These Markdown/MDX sources also produce the website; examples are inline.",
        "",
        "## Documentation",
        "",
      ]
      for (const page of docsPages) {
        const path = resolve(docs, page.file)
        const source = documentationSource(readFileSync(path, "utf8"), path)
        this.addWatchFile(path)
        this.emitFile({ type: "asset", fileName: `docs/source/${page.file}`, source })
        index.push(`- [${page.title}](${base}docs/source/${page.file})`)
      }
      this.emitFile({ type: "asset", fileName: "llms.txt", source: index.join("\n") + "\n" })
    },
  }
}
