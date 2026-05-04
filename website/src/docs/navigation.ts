export interface DocsNavigationItem {
  readonly title: string
  readonly slug: string
}

export interface DocsNavigationGroup {
  readonly title: string
  readonly items: readonly DocsNavigationItem[]
}

export type DocsNavigationEntry = DocsNavigationItem | DocsNavigationGroup

export interface DocsNavigationSection {
  readonly title: string
  readonly items: readonly DocsNavigationEntry[]
}

export const docsNavigation = [
  {
    title: "Overview",
    items: [{ title: "Introduction", slug: "" }],
  },
  {
    title: "Domain Macros",
    items: [
      { title: "Overview", slug: "domain-macros" },
      {
        title: "Declarations",
        items: [
          {
            title: "install_macro_support!",
            slug: "domain-macros/install-macro-support",
          },
          { title: "BoundedContext", slug: "domain-macros/bounded-context" },
          { title: "Aggregate", slug: "domain-macros/aggregate" },
          {
            title: "AggregateEvents",
            slug: "domain-macros/aggregate-events",
          },
          { title: "Entity", slug: "domain-macros/entity" },
          {
            title: "DomainIdentity",
            slug: "domain-macros/domain-identity",
          },
          { title: "ValueObject", slug: "domain-macros/value-object" },
          { title: "DomainService", slug: "domain-macros/domain-service" },
          { title: "Command", slug: "domain-macros/command" },
          { title: "DomainEvent", slug: "domain-macros/domain-event" },
          { title: "DomainError", slug: "domain-macros/domain-error" },
          {
            title: "DecisionOutcome",
            slug: "domain-macros/decision-outcome",
          },
          {
            title: "EntityLifecycle",
            slug: "domain-macros/entity-lifecycle",
          },
          {
            title: "StateTransition",
            slug: "domain-macros/state-transition",
          },
          { title: "domain_model!", slug: "domain-macros/domain-model" },
        ],
      },
      {
        title: "Behaviour",
        items: [
          { title: "domain_action", slug: "domain-macros/domain-action" },
          { title: "domain_query", slug: "domain-macros/domain-query" },
          {
            title: "domain_decision",
            slug: "domain-macros/domain-decision",
          },
          {
            title: "domain_invariant",
            slug: "domain-macros/domain-invariant",
          },
          { title: "domain_*_test", slug: "domain-macros/domain-tests" },
        ],
      },
    ],
  },
  {
    title: "Project Structure",
    items: [
      { title: "Overview", slug: "project-structure" },
      { title: "Domain root", slug: "project-structure/domain-root" },
      {
        title: "Bounded context",
        slug: "project-structure/bounded-context",
      },
      { title: "Aggregate", slug: "project-structure/aggregate" },
      { title: "Entity", slug: "project-structure/entity" },
      {
        title: "Simple value object",
        slug: "project-structure/simple-value-object",
      },
      {
        title: "Behaviorful value object",
        slug: "project-structure/behaviorful-value-object",
      },
      {
        title: "Domain service",
        slug: "project-structure/domain-service",
      },
      {
        title: "Aggregate action",
        slug: "project-structure/aggregate-action",
      },
      {
        title: "Entity action",
        slug: "project-structure/entity-action",
      },
      {
        title: "Value-object action",
        slug: "project-structure/value-object-action",
      },
      { title: "Query", slug: "project-structure/query" },
      {
        title: "Aggregate decision",
        slug: "project-structure/aggregate-decision",
      },
      {
        title: "Value-object decision",
        slug: "project-structure/value-object-decision",
      },
      {
        title: "Aggregate invariant",
        slug: "project-structure/aggregate-invariant",
      },
      {
        title: "Value-object invariant",
        slug: "project-structure/value-object-invariant",
      },
      {
        title: "Entity lifecycle",
        slug: "project-structure/entity-lifecycle",
      },
    ],
  },
] as const satisfies readonly DocsNavigationSection[]

type SlugFromNavigation<T> = T extends {
  readonly slug: infer Slug extends string
}
  ? Slug
  : T extends { readonly items: readonly (infer Item)[] }
    ? SlugFromNavigation<Item>
    : never

export type DocsSlug = SlugFromNavigation<(typeof docsNavigation)[number]>
