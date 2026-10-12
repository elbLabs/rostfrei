import { docsNavigation } from "./docs/navigation.ts"
import type { DocsNavigationEntry } from "./docs/navigation.ts"

export const site = {
  name: "Rostfrei",
  title: "Rostfrei · Understand your code, even when AI writes it",
  description:
    "Keep AI-written Rust understandable. Rostfrei connects explicit domain ownership, event history, and traceable command flows for humans and coding agents.",
  url: "https://elblabs.github.io/rostfrei/",
}

export interface PageMetadata {
  path: string
  title: string
  description: string
  noindex?: boolean
}

const introductions: Record<
  string,
  Pick<PageMetadata, "title" | "description">
> = {
  "": {
    title: "Introduction · Rostfrei docs",
    description:
      "Understand a Rostfrei codebase, even when AI agents wrote it. Find business rules, inspect domain contracts, and follow commands through history and integrations.",
  },
  "getting-started": {
    title: "Getting started · Rostfrei docs",
    description:
      "Run the Rostfrei bike-rental example. Inspect its domain model, start the NATS-backed application, and explore command flows in Tracer Studio.",
  },
  "messaging/subjects": {
    title: "Subjects and streams · Rostfrei docs",
    description:
      "Follow a bicycle rental through NATS subjects, durable commands, private domain-event history, public integration events, and isolated Test resources.",
  },
  "domain-macros": {
    title: "Domain macros · Rostfrei docs",
    description:
      "Describe domain types and behavior contracts in ordinary Rust. Explore Rostfrei macros for aggregates, commands, events, policies, and model composition.",
  },
  "project-structure": {
    title: "Project structure · Rostfrei docs",
    description:
      "Give every domain concept a predictable home. Learn how Rostfrei's checked project structure connects declarations, ownership, and behavior implementations.",
  },
  "read-models": {
    title: "Read models · Rostfrei docs",
    description:
      "Declare saved query views in Rust and register domain and integration event handlers. Rostfrei handles KV persistence, checkpoints, and concurrency retries.",
  },
  "read-models/nats": {
    title: "Read-model NATS setup · Rostfrei docs",
    description:
      "Provision scoped NATS KV storage, configure the read-model backend, and run ordered event consumers using the application's managed connection.",
  },
  "read-models/ordering": {
    title: "Read-model ordering and recovery · Rostfrei docs",
    description:
      "Understand default broker ordering, optional business versions, duplicate delivery, quarantine recovery, and rebuilding read-model generations.",
  },
  "read-models/benchmarks": {
    title: "Read-model query benchmarks · Rostfrei docs",
    description:
      "Compare KV queries with replaying two connected aggregates. Explore recorded latency, broker request counts, timing boundaries, and reproduction commands.",
  },
}

function pagesForEntries(
  entries: readonly DocsNavigationEntry[]
): PageMetadata[] {
  return entries.flatMap((entry) => {
    if ("items" in entry) return pagesForEntries(entry.items)

    const isMacro = entry.slug.startsWith("domain-macros/")
    return [
      {
        path: entry.slug ? `/docs/${entry.slug}` : "/docs",
        ...(introductions[entry.slug] ?? {
          title: `${entry.title} ${isMacro ? "macro" : "structure"} · Rostfrei docs`,
          description: isMacro
            ? `Learn how ${entry.title} describes a Rust domain contract in Rostfrei, with authored code and a simplified view of the generated implementation.`
            : `Learn where ${entry.title.toLowerCase()} declarations and implementations belong in a Rostfrei project, with checked ownership conventions and examples.`,
        }),
      },
    ]
  })
}

export const sitePages: PageMetadata[] = [
  { path: "/", title: site.title, description: site.description },
  ...docsNavigation.flatMap((section) => pagesForEntries(section.items)),
]

export function metadataForPath(pathname: string): PageMetadata {
  const path = pathname.replace(/\/+$/, "") || "/"
  return (
    sitePages.find((page) => page.path === path) ?? {
      path,
      title: "Page not found · Rostfrei",
      description:
        "Find Rostfrei documentation, the getting-started guide, and domain-modeling references.",
      noindex: true,
    }
  )
}

export interface PageHeadTag {
  tag: "title" | "meta" | "link"
  attrs: Record<string, string>
  children?: string
}

// Shared by the static HTML build and client-side navigation.
export function pageHeadTags(page: PageMetadata): PageHeadTag[] {
  const url = new URL(page.path.replace(/^\//, ""), site.url).href
  const tags: PageHeadTag[] = [
    { tag: "title", attrs: {}, children: page.title },
    { tag: "meta", attrs: { name: "description", content: page.description } },
    { tag: "meta", attrs: { property: "og:site_name", content: site.name } },
    { tag: "meta", attrs: { property: "og:type", content: "website" } },
    { tag: "meta", attrs: { property: "og:title", content: page.title } },
    {
      tag: "meta",
      attrs: { property: "og:description", content: page.description },
    },
    { tag: "meta", attrs: { name: "twitter:card", content: "summary" } },
    { tag: "meta", attrs: { name: "twitter:title", content: page.title } },
    {
      tag: "meta",
      attrs: { name: "twitter:description", content: page.description },
    },
  ]

  if (page.noindex) {
    tags.push({ tag: "meta", attrs: { name: "robots", content: "noindex" } })
  } else {
    tags.push(
      { tag: "link", attrs: { rel: "canonical", href: url } },
      { tag: "meta", attrs: { property: "og:url", content: url } }
    )
  }

  return tags.map((tag) => ({
    ...tag,
    attrs: { ...tag.attrs, "data-rostfrei-meta": "" },
  }))
}
