# Rostfrei documentation

This is the official documentation for developers and coding agents. The website
renders these same files; read them here, in a checkout, or on the
[documentation site](https://elblabs.github.io/rostfrei/docs).
The published [machine index](https://elblabs.github.io/rostfrei/llms.txt) links to
the same Markdown/MDX sources without requiring JavaScript.

## Guides

- [Introduction](introduction.mdx)
- [Getting started](getting-started.mdx)
- [Domain macros](domain-macros/index.mdx) and [project structure](project-structure/index.mdx)
- [Bike-rental setup and Tracer workflow](../examples/bike-rental/README.md)
- [Tracer Studio](../studio/README.md)
- [Agent Skills setup and use](agent-skills-install.md)
- [Project terminology](glossary.md)
- [Release changes](../CHANGELOG.md)
- [Migrating to 0.1](migration-0.1.md)
- [Development and releases](development.md)

## Runtime and operations

- [NATS authentication, event history, and command metadata](runtime.md)
- [Command runtime and outcome lifecycle](command-runtime.md)
- [Technical failures and redrive](command-failures.md)
- [Original actor attribution](actor-attribution.md)
- [Completed-outcome cutover](completed-outcome-cutover.md)
- [Application-linked NATS host](nats-application-host.md)
- [Worker concurrency and shared capacity](consumer-concurrency.md)
- [Command-response timeouts](command-response-timeout.md)
- [Subjects and streams](messaging/subjects.mdx)
- [Message schemas and subscription upgrades](messaging-upgrades.md)
- [Event-driven read models](read-models/index.mdx), [NATS setup](read-models/nats.mdx),
  [ordering and recovery](read-models/ordering.mdx), and [typed storage](read-models.md)
- [Continuous observation](continuous-observation.md)
- [Handler simulation](handler-simulation.md)
- [Quarantine inspection](quarantine-inspection.md) and [example walkthrough](../examples/bike-rental/QUARANTINE.md)
- [Domain-event workers and durable migration](domain-event-workers.md)
- [Aggregate-loading benchmarks](benchmarks/aggregate-loading.md) and [read-model benchmarks](benchmarks/read-models.md)
- [Skills release qualification](release-01-kit.md)

## Workspace crates

Applications normally depend on `rostfrei`, enabling features for the adapters
they need. See [dependency setup and imports](getting-started.mdx#one-framework-dependency).

| Crate | Responsibility |
| --- | --- |
| [`rostfrei`](../crates/rostfrei) | Application facade, typed buses, runtime, registry, and public macros. |
| [`rostfrei-runtime`](../crates/rostfrei-runtime) | Transport-independent application runtime shared by the facade and adapters. |
| [`rostfrei-http`](../crates/rostfrei-http) | Explicitly mounted HTTP command and query routes. |
| [`rostfrei-tracer`](../crates/rostfrei-tracer) | Command discovery, real Test execution, production dispatch, behavioral tests, and bounded observation. |
| [`rostfrei-core`](../crates/rostfrei-core) | Aggregate execution, event-store contracts, and the in-memory store. |
| [`rostfrei-domain`](../crates/rostfrei-domain) | Compiled domain model, descriptors, ownership rules, and domain-test metadata; imported as `domain`. |
| [`rostfrei-domain-macros`](../crates/rostfrei-domain-macros) | Domain derives and behavior attributes. |
| [`rostfrei-domain-runtime`](../crates/rostfrei-domain-runtime) | Initialization, event application, and runtime registration for compiled domain types. |
| [`rostfrei-fixtures`](../crates/rostfrei-fixtures) | MessageSeries fixture validation and deterministic event-store replay. |
| [`rostfrei-structure`](../crates/rostfrei-structure) | Filesystem checks, project scaffolding, and the `cargo-rostfrei` CLI. |
| [`rostfrei-registry`](../crates/rostfrei-registry) | Command and query metadata and runtime registration. |
| [`rostfrei-macros`](../crates/rostfrei-macros) | `QueryDefinition` and `ReadModel` derives for application queries and read models. |
| [`rostfrei-messaging-core`](../crates/rostfrei-messaging-core) | Transport-neutral messages, envelopes, and delivery contracts. |
| [`rostfrei-nats`](../crates/rostfrei-nats) | NATS messaging adapters, JetStream event storage, and KV-backed read models. |
| [`rostfrei-testing`](../crates/rostfrei-testing) | Reusable event-store contracts and aggregate scenarios. |

The workspace also includes the [`bike-rental`](../examples/bike-rental) example
and [`rostfrei-benchmarks`](../crates/rostfrei-benchmarks/README.md), a
non-published tool for release workloads, comparisons, and disposable NATS
orchestration.

## Architecture

The [architectural maxims](adr/README.md) guide implementation and PR review:
business meaning, durable decisions, authoritative history, explicit contracts,
derived views, and evidence. Each ADR gives the principle, rationale, and tradeoff.

## Maintaining these docs

Update the relevant page when behavior changes. Keep concepts, examples, and
operating instructions here; keep work logs in issues and PRs. Use relative links
between source files—the website resolves them to its routes. MDX may add visual
enhancements, but essential explanations and code must remain readable as text.
Agent Skills describe workflows and use these same contracts.
