import type { MDXModule } from "mdx/types"

import { docsPages } from "./navigation"
import type { DocsSlug } from "./navigation"

export type DocsPageModule = MDXModule
export type DocsPageLoader = () => Promise<DocsPageModule>

const sources = import.meta.glob<DocsPageModule>("../../../docs/**/*.{md,mdx}")

export const docsRegistry: Record<string, DocsPageLoader> = Object.fromEntries(
  docsPages.map(({ slug, file }) => {
    const load = sources[`../../../docs/${file}`]
    if (!load) throw new Error(`Missing documentation source: ${file}`)
    return [slug, load]
  })
)

export function isDocsSlug(slug: string): slug is DocsSlug {
  return Object.prototype.hasOwnProperty.call(docsRegistry, slug)
}

export async function loadDocsPage(
  slug: string
): Promise<DocsPageModule | undefined> {
  if (!isDocsSlug(slug)) return undefined
  return docsRegistry[slug]()
}
