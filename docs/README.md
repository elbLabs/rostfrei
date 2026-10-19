# Reference index

## Guides

- [Domain macros and project structure](https://elblabs.github.io/rostfrei/docs)
- [Getting started](https://elblabs.github.io/rostfrei/docs/getting-started)
- [Bike-rental setup and Tracer workflow](../examples/bike-rental/README.md)
- [Tracer Studio](../studio/README.md)
- [Agent Skills setup and use](agent-skills-install.md)
- [Project terminology](../UBIQUITOUS_LANGUAGE.md)
- [Release changes](../CHANGELOG.md)
- [Migrating to 0.1](migration-0.1.md)
- [Development and releases](development.md)

## Runtime and operations

- [NATS authentication, event history, and command metadata](runtime.md)
- [Completed-outcome cutover](completed-outcome-cutover.md)
- [Application-linked NATS host](nats-application-host.md)
- [Worker concurrency and shared capacity](consumer-concurrency.md)
- [Command-response timeouts](command-response-timeout.md)
- [Subjects and streams](https://elblabs.github.io/rostfrei/docs/messaging/subjects)
  ([offline HTML guide](subjects.html))
- [Event-driven read models](read-model-runtime.md) and [typed storage](read-models.md)
- [Evolving private domain events](event-schema-evolution.md)
- [Continuous observation](continuous-observation.md)
- [Quarantine inspection](quarantine-inspection.md) and [example walkthrough](../examples/bike-rental/QUARANTINE.md)
- [Domain-event worker pool](design/domain-event-worker-pool.md)
- [Aggregate-loading benchmarks](benchmarks/aggregate-loading.md) and [read-model benchmarks](read-model-benchmark.md)

## Workspace crates

Applications normally depend on `rostfrei` and the adapters they need.

| Crate | Responsibility |
| --- | --- |
| [`rostfrei`](../crates/rostfrei) | Application facade, typed buses, runtime, registry, and public macros. |
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

## Architecture and history

[Architecture decision records](adr) explain how the design evolved. Earlier
records can contain APIs replaced by later decisions; use the guides and current
source for implementation examples. Useful entry points:

- [Language and scope](adr/0001-ubiquitous-language-and-scope.md)
- [Runtime layers](adr/0008-development-platform-layers.md)
- [Compiled domain model](adr/0013-compiled-domain-model.md)
- [Application-scoped NATS conventions](adr/0014-application-scoped-nats-conventions.md)
- [Isolated test traffic](adr/0018-opinionated-test-traffic-scope.md)
- [HTTP commands and queries](adr/0019-registered-queries-and-standard-http.md)
- [MessageSeries fixtures](adr/0020-message-series-fixtures.md)
- [Domain policies](adr/0036-domain-policy-vocabulary.md)
- [Bounded-context commands and transactions](adr/0037-bounded-context-commands-and-unit-of-work.md)

The [10 September 2026 review](reviews/2026-09-10-project-review.md) records
findings and checks against its stated commit, including open follow-ups. It is
a historical review, not a release-readiness report for the current checkout.
