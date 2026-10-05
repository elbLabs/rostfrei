import type { Plugin } from "vite"

import { pageHeadTags, sitePages } from "./src/site.ts"
import type { PageMetadata } from "./src/site.ts"

function escapeHtml(value: string) {
  return value.replace(/[&<>"']/g, (character) => {
    const entities: Record<string, string> = {
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;",
    }
    return entities[character]
  })
}

function withMetadata(html: string, page: PageMetadata) {
  const tags = pageHeadTags(page)
    .map(({ tag, attrs, children }) => {
      const attributes = Object.entries(attrs)
        .map(([key, value]) => `${key}="${escapeHtml(value)}"`)
        .join(" ")
      return tag === "title"
        ? `<title ${attributes}>${escapeHtml(children ?? "")}</title>`
        : `<${tag} ${attributes} />`
    })
    .join("\n    ")

  return html.replace(
    /<!-- page-metadata:start -->[\s\S]*?<!-- page-metadata:end -->/,
    `<!-- page-metadata:start -->\n    ${tags}\n    <!-- page-metadata:end -->`
  )
}

// GitHub Pages serves a real HTML entry for each documented URL. Link unfurlers
// get the correct metadata without JavaScript; the existing React app renders it.
export function metadataPlugin(): Plugin {
  return {
    name: "rostfrei-page-metadata",
    transformIndexHtml(html) {
      return withMetadata(html, sitePages[0])
    },
    generateBundle: {
      order: "post",
      handler(_options, bundle) {
        const index = bundle["index.html"]
        if (
          !index ||
          index.type !== "asset" ||
          typeof index.source !== "string"
        ) {
          throw new Error("The website build did not emit index.html")
        }

        for (const page of sitePages) {
          if (page.path === "/") continue
          this.emitFile({
            type: "asset",
            fileName: `${page.path.slice(1)}/index.html`,
            source: withMetadata(index.source, page),
          })
        }
      },
    },
  }
}
