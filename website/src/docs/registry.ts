import type { MDXModule } from "mdx/types"

import type { DocsSlug } from "./navigation"

export type DocsPageModule = MDXModule
export type DocsPageLoader = () => Promise<DocsPageModule>

export const docsRegistry = {
  "": () => import("@/content/docs/introduction.mdx"),
  "domain-macros": () => import("@/content/docs/domain-macros/index.mdx"),
  "domain-macros/install-macro-support": () =>
    import("@/content/docs/domain-macros/install-macro-support.mdx"),
  "domain-macros/bounded-context": () =>
    import("@/content/docs/domain-macros/bounded-context.mdx"),
  "domain-macros/aggregate": () =>
    import("@/content/docs/domain-macros/aggregate.mdx"),
  "domain-macros/aggregate-events": () =>
    import("@/content/docs/domain-macros/aggregate-events.mdx"),
  "domain-macros/entity": () =>
    import("@/content/docs/domain-macros/entity.mdx"),
  "domain-macros/domain-identity": () =>
    import("@/content/docs/domain-macros/domain-identity.mdx"),
  "domain-macros/value-object": () =>
    import("@/content/docs/domain-macros/value-object.mdx"),
  "domain-macros/domain-service": () =>
    import("@/content/docs/domain-macros/domain-service.mdx"),
  "domain-macros/command": () =>
    import("@/content/docs/domain-macros/command.mdx"),
  "domain-macros/domain-event": () =>
    import("@/content/docs/domain-macros/domain-event.mdx"),
  "domain-macros/domain-error": () =>
    import("@/content/docs/domain-macros/domain-error.mdx"),
  "domain-macros/decision-outcome": () =>
    import("@/content/docs/domain-macros/decision-outcome.mdx"),
  "domain-macros/entity-lifecycle": () =>
    import("@/content/docs/domain-macros/entity-lifecycle.mdx"),
  "domain-macros/state-transition": () =>
    import("@/content/docs/domain-macros/state-transition.mdx"),
  "domain-macros/domain-action": () =>
    import("@/content/docs/domain-macros/domain-action.mdx"),
  "domain-macros/domain-query": () =>
    import("@/content/docs/domain-macros/domain-query.mdx"),
  "domain-macros/domain-decision": () =>
    import("@/content/docs/domain-macros/domain-decision.mdx"),
  "domain-macros/domain-invariant": () =>
    import("@/content/docs/domain-macros/domain-invariant.mdx"),
  "domain-macros/domain-tests": () =>
    import("@/content/docs/domain-macros/domain-tests.mdx"),
  "domain-macros/domain-model": () =>
    import("@/content/docs/domain-macros/domain-model.mdx"),
  "project-structure": () =>
    import("@/content/docs/project-structure/index.mdx"),
  "project-structure/domain-root": () =>
    import("@/content/docs/project-structure/domain-root.mdx"),
  "project-structure/bounded-context": () =>
    import("@/content/docs/project-structure/bounded-context.mdx"),
  "project-structure/aggregate": () =>
    import("@/content/docs/project-structure/aggregate.mdx"),
  "project-structure/entity": () =>
    import("@/content/docs/project-structure/entity.mdx"),
  "project-structure/simple-value-object": () =>
    import("@/content/docs/project-structure/simple-value-object.mdx"),
  "project-structure/behaviorful-value-object": () =>
    import("@/content/docs/project-structure/behaviorful-value-object.mdx"),
  "project-structure/domain-service": () =>
    import("@/content/docs/project-structure/domain-service.mdx"),
  "project-structure/aggregate-action": () =>
    import("@/content/docs/project-structure/aggregate-action.mdx"),
  "project-structure/entity-action": () =>
    import("@/content/docs/project-structure/entity-action.mdx"),
  "project-structure/value-object-action": () =>
    import("@/content/docs/project-structure/value-object-action.mdx"),
  "project-structure/query": () =>
    import("@/content/docs/project-structure/query.mdx"),
  "project-structure/aggregate-decision": () =>
    import("@/content/docs/project-structure/aggregate-decision.mdx"),
  "project-structure/entity-decision": () =>
    import("@/content/docs/project-structure/entity-decision.mdx"),
  "project-structure/value-object-decision": () =>
    import("@/content/docs/project-structure/value-object-decision.mdx"),
  "project-structure/aggregate-invariant": () =>
    import("@/content/docs/project-structure/aggregate-invariant.mdx"),
  "project-structure/value-object-invariant": () =>
    import("@/content/docs/project-structure/value-object-invariant.mdx"),
  "project-structure/entity-lifecycle": () =>
    import("@/content/docs/project-structure/entity-lifecycle.mdx"),
} satisfies Record<DocsSlug, DocsPageLoader>

export function isDocsSlug(slug: string): slug is DocsSlug {
  return Object.prototype.hasOwnProperty.call(docsRegistry, slug)
}

export async function loadDocsPage(
  slug: string
): Promise<DocsPageModule | undefined> {
  if (!isDocsSlug(slug)) return undefined

  return docsRegistry[slug]()
}
