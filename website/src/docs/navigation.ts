export interface DocsNavigationItem {
  readonly title: string
  readonly slug: string
  readonly file?: string
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
    title: "Start here",
    items: [
      { title: "Introduction", slug: "" },
      { title: "Documentation index", slug: "reference", file: "README.md" },
      { title: "Getting started", slug: "getting-started" },
      { title: "Terminology", slug: "glossary", file: "glossary.md" },
    ],
  },
  {
    title: "Messaging",
    items: [
      { title: "Subjects and streams", slug: "messaging/subjects" },
      { title: "Schema upgrades", slug: "messaging-upgrades", file: "messaging-upgrades.md" },
      { title: "Domain-event workers", slug: "domain-event-workers", file: "domain-event-workers.md" },
      { title: "Worker capacity", slug: "consumer-concurrency", file: "consumer-concurrency.md" },
    ],
  },
  {
    title: "Domain macros",
    items: [
      { title: "Overview", slug: "domain-macros", file: "domain-macros/index.mdx" },
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
            title: "PolicyOutcome",
            slug: "domain-macros/policy-outcome",
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
        title: "Behavior",
        items: [
          { title: "domain_action", slug: "domain-macros/domain-action" },
          { title: "domain_query", slug: "domain-macros/domain-query" },
          {
            title: "domain_policy",
            slug: "domain-macros/domain-policy",
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
    title: "Read models",
    items: [
      { title: "Declare and query", slug: "read-models", file: "read-models/index.mdx" },
      { title: "NATS setup", slug: "read-models/nats" },
      { title: "Ordering and recovery", slug: "read-models/ordering" },
      { title: "Typed storage", slug: "read-models/storage", file: "read-models.md" },
      { title: "Benchmarks", slug: "read-models/benchmarks", file: "benchmarks/read-models.md" },
    ],
  },
  {
    title: "Project structure",
    items: [
      { title: "Overview", slug: "project-structure", file: "project-structure/index.mdx" },
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
        title: "Aggregate policy",
        slug: "project-structure/aggregate-policy",
      },
      {
        title: "Entity policy",
        slug: "project-structure/entity-policy",
      },
      {
        title: "Value-object policy",
        slug: "project-structure/value-object-policy",
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
        title: "Lifecycle",
        slug: "project-structure/entity-lifecycle",
      },
    ],
  },
  {
    title: "Runtime",
    items: [
      { title: "Command execution", slug: "command-runtime", file: "command-runtime.md" },
      { title: "NATS operations", slug: "runtime", file: "runtime.md" },
      { title: "Application host", slug: "nats-application-host", file: "nats-application-host.md" },
      { title: "Actor attribution", slug: "actor-attribution", file: "actor-attribution.md" },
      { title: "Response timeouts", slug: "command-response-timeout", file: "command-response-timeout.md" },
      { title: "Failure recovery", slug: "command-failures", file: "command-failures.md" },
      { title: "Deployment cutover", slug: "completed-outcome-cutover", file: "completed-outcome-cutover.md" },
      { title: "Migration to 0.1", slug: "migration-0.1", file: "migration-0.1.md" },
    ],
  },
  {
    title: "Tracer and agents",
    items: [
      { title: "Agent Skills", slug: "agent-skills-install", file: "agent-skills-install.md" },
      { title: "Observation", slug: "continuous-observation", file: "continuous-observation.md" },
      { title: "Quarantine", slug: "quarantine-inspection", file: "quarantine-inspection.md" },
      { title: "Handler simulation", slug: "handler-simulation", file: "handler-simulation.md" },
    ],
  },
  {
    title: "Architecture",
    items: [
      { title: "Maxims", slug: "adr", file: "adr/README.md" },
      { title: "Domain model", slug: "adr/domain-model", file: "adr/domain-model.md" },
      { title: "Command execution", slug: "adr/command-execution", file: "adr/command-execution.md" },
      { title: "NATS persistence", slug: "adr/nats-storage", file: "adr/nats-storage.md" },
      { title: "Messaging", slug: "adr/messaging", file: "adr/messaging.md" },
      { title: "Read models", slug: "adr/read-models", file: "adr/read-models.md" },
      { title: "Tracer", slug: "adr/tracer", file: "adr/tracer.md" },
    ],
  },
  {
    title: "Contributing",
    items: [
      { title: "Development and releases", slug: "development", file: "development.md" },
      { title: "Skills qualification", slug: "release-01-kit", file: "release-01-kit.md" },
      { title: "Aggregate benchmarks", slug: "benchmarks/aggregate-loading", file: "benchmarks/aggregate-loading.md" },
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

function pagesFor(entries: readonly DocsNavigationEntry[]): {
  title: string
  slug: string
  file: string
}[] {
  return entries.flatMap((entry) =>
    "items" in entry
      ? pagesFor(entry.items)
      : [{ ...entry, file: entry.file ?? `${entry.slug || "introduction"}.mdx` }]
  )
}

export const docsPages = docsNavigation.flatMap((section) => pagesFor(section.items))
