# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

### Added

- Framework-owned **correlation index** ([ADR 0045](docs/adr/0045-correlation-index-and-anchored-views.md)):
  one application-scoped KV read model maps each correlation to references of
  its committed domain events and published integration events across all
  bounded contexts. It reads metadata only, never payloads.
  `NatsCorrelationIndexProjector` indexes one bounded context;
  `NatsCorrelationIndexReader` returns ordered chains and per-context `lag`
  computed against the source streams. A correlation keeps 2048 references and
  then reports `truncated`. The bucket has no TTL and rejects writes when full;
  deleting and provisioning it again rebuilds the index from the event stores.
- `NatsReadModelConfig::application_scoped` derives a bucket without the bounded
  context segment and keeps the traffic scope.
- `AggregateDefinition::ANCHOR` declares an aggregate as an anchor. The compiled
  domain model projects it as `"anchor": true`.
- `NatsRuntimeConfig::with_correlation_index` and
  `NatsApplicationBuilder::with_correlation_index` provision the index and run
  its projector as a supervised worker. Tracer serves
  `GET /aggregates/{type}/{id}/correlations` and indexed
  `GET /correlations/{id}/events` to the production inspection capability and
  lists anchor types in the catalog. See [correlation index and anchored views](docs/correlation-index.md).
- An anchored view of an ID without a stored event returns
  `404 unknown-anchor-stream`; `unknown-anchor` is reserved for types that are
  not anchors.
- Bike-rental indexes its Normal runtime and declares the rental fleet an anchor.
- `CorrelationIndexReader::freshness` replaces `lag`. It reports every source
  with its stream, `active`/`inactive` state and `behind` count; `lag[context]`
  is `null` while any source of the context has no running projector. Tracer
  index views add `sources`, and `GET /correlations/{id}/events` returns `404`
  only when every source is active and caught up.
- The projector names its ordered consumers
  `correlation-index--<kind>--<context>--<uuid v7>` and deletes them on shutdown,
  failure and abort. Readers additionally need `CONSUMER.NAMES` on the source
  streams.
- `NatsCorrelationIndexProjector::connect_domain_only` indexes a context
  without integration events. It refuses a context whose integration source is
  already registered; rebuild the index to drop it.
- The correlation index projector rejects source streams with work-queue or
  interest retention or a consumer limit at startup (`ConfigurationMismatch`).
  Its unacknowledged consumers would remove their messages or exhaust the limit.
- Correlation index sources detect a recreated stream by sequence regression
  instead of the stream's `created` time, which JetStream changes after a server
  restart for an updated stream. That drift no longer fails freshness reads or
  turns the Tracer views into `503`. Messages a Limits stream removed before
  they were indexed are reported per source as `skipped`; projection continues.
  Stored source records with a `generation` field remain readable.
- Anchored correlation views accept repeated
  `match=<context>/<event-type>.<field>=<value>` filters that keep only
  correlations whose loaded payloads carry the value, AND-ed across filters,
  and echo them as `matched`. A filter on a context that is neither the
  anchor's nor included returns `400 match-requires-include`; a malformed one
  `400 invalid-match`. No index change.
- Tracer Studio **Correlations** view: pick an advertised anchor and aggregate
  ID to list its correlations with event counts, contexts and first/last seen,
  or open a correlation ID directly. A selected chain renders indexed domain
  and integration events with their causation, bounded context and per-context
  index freshness; unknown lag is shown as unknown, never as current. It uses
  `VITE_TRACER_INSPECTION_TOKEN` and explains unknown anchor streams, empty
  anchors and correlations that are not indexed yet.

### Changed

- **Breaking (read-model policy):** `NatsReadModelConfig::context()` returns
  `Option<&BoundedContext>`, `None` for application-scoped models. `application()`
  and `traffic_scope()` return the scope of every model.

### Fixed

- The NATS application host rejects several integration handlers for one address,
  for example typed v1 and v2 handlers of the same contract. They previously
  shared one durable and quarantined each other's deliveries.

## [0.1.1] - 2026-10-19

### Changed

- Prepare all 14 public framework/tooling crates for crates.io with descriptions,
  shared repository/homepage/documentation links and a packaged README. Benchmark
  and example applications remain private workspace packages.
- Align the Git release, CLI, Agent Skills kit and registry packages on one 0.1.1
  source revision. Runtime behavior is unchanged from 0.1.0.
- Document registry installation and dependency-ordered workspace publication.
  The generated starter's historical Git pin remains independent of publication.

## [0.1.0] - 2026-10-19

See the [0.1 migration guide](docs/migration-0.1.md) before upgrading an existing
application or NATS authority.

### Added

- Modular command execution with a coordinated `ExecutionStore`, consuming commit
  sessions, permanent completed-decision markers and separately recoverable result
  details. Accepted, rejected and event-free outcomes remain idempotent across
  direct, local, NATS and HTTP execution, including with conflict retries disabled.
- Object-safe `CommandProcessingService` with per-call trusted context, explicit
  authorization, optional bounded audit delivery, shared event/rejection codecs
  and authorized completed-result lookup. `ActorRef` preserves original attribution
  independently of the authenticated principal and security scope.
- Incarnation-fenced NATS journal/outcome-state authority pairs, verified result
  recovery and explicit detail expiry. Expiry withdraws result details while
  retaining the decision and event-integrity evidence; it never permits reevaluation.
- Application-linked `NatsApplication`/`NatsRuntime` host with shared bindings,
  supervised workers, optional Tracer HTTP, configured execution profiles and
  paired Test reset. The bike-rental entry point enables Tracer with `--tracer`.
- Dependency-aware native Agent Skills setup and portable, checksummed release
  kits. `cargo rostfrei skills install/update` follows the application's locked
  framework revision, preserves managed-file conflicts, and retains the matching
  native runner for source-free execution. The development skill guides actual
  NATS-backed Given/When/Then red/green work and a visible evidence walkthrough.
- Opt-in continuous observation of application-originated domain and integration
  events, with scope-isolated retention, health, redacted message series and
  read-only list/detail/SSE discovery. Studio adds Test/Production observation,
  filtering, pause/resume, reconnection and retention diagnostics.
- Non-published `rostfrei-benchmarks` workspace package owning aggregate readiness,
  typed read-model query workloads and paired history comparisons. Its Rust runner
  manages disposable pinned NATS, cancellation/cleanup, subprocess failures and
  report validation without Python. Benchmark-specific Python tooling is removed.
- Typed, revision-aware `ReadModelStore<T>` and NATS KV adapter with versioned
  codecs, CAS create/update/delete, application/context/model/traffic isolation,
  and explicit provisioning, verification, and policy updates. Includes a
  multi-handler query/rebuild example and migration/operations documentation.
- `ReadModel` declarations and event-only registration builders with automatic
  persistence, source checkpoints, bounded CAS retries, authoritative domain-gap
  catch-up, broker-default integration ordering with optional business versions,
  read-only query handles, and managed NATS consumer workers.
- `NatsEventStore::with_history_auditing(bool)` selects historical auditing per
  handle. It defaults to off and never disables local integrity or operation-specific
  write/retry checks; explicit audit methods always audit.

### Changed

- **Breaking (runtime ports and results):** executor backends implement
  `ExecutionStore`; the pre-release `CommandBackend` name is retired.
  `CompletedOutcome<T>` carries a permanent marker and detail availability;
  `InlineCompletedOutcome` names the explicit legacy full-result wrapper.
  Completed-result recovery failures remain technical failures, not permission
  to evaluate a completed operation again.
- **Breaking (NATS command authority):** new coordinated execution requires explicit
  fresh split-outcome authority. Legacy history stays fail-closed for unevidenced
  execution. Journal and state resources, their incarnations and the configured
  epoch are one authority; an epoch change is an operator-managed cutover.
- **Command-response timeout:** `NatsMessagingAdapter::new` now defaults to a
  30-second response-reading budget instead of waiting indefinitely.
  `with_response_timeout` remains configurable. Publication, observers and replay
  admission precede the budget; a timeout is an unconfirmed outcome, not
  cancellation or automatic republish.
- **Consumer capacity:** `ConsumerConfig::concurrency()` bounds one worker's
  handlers. `with_shared_pending_limit` independently sizes the durable's shared
  unacknowledged-message budget, including prefetch. Its default remains local
  concurrency for compatibility; it supplies no per-worker reservation or fairness.
- Add explicit **Simulate handler** diagnostics through the control catalog's
  `simulateHandlerHrefTemplate` and POST `/simulate-handler`: real registered
  handler computation with starting versions, typed decision and proposed,
  uncommitted events. Studio displays a separate diagnostic result; Test remains
  primary. There is no operation, synthetic causal trace or durable acceptance
  receipt. Injected dependencies are not automatically sandboxed.
- **Breaking (Tracer Preview retirement):** remove simulation discovery and HTTP
  routes, Studio's Preview action and synthetic operation generation. Retired
  requests return 404 rather than publishing to Test or production. The legacy
  Rust submission method returns `ModeUnavailable("simulate")`; the new handler-only
  interface above replaces useful diagnostics, not legacy operation semantics. Core
  `CommandExecutor::simulate`/`SimulationOutcome` remain supported for focused
  tests. Historical simulation evidence remains explicitly uncommitted.
  Catalog stays v1 (capability discovery); behavior/MessageSeries v2 and fixture
  v1 are unchanged. Release-matched clients must handle missing action links.
- NATS aggregate histories now replay in bounded, exact-subject batches instead of
  issuing one broker request per event. Reads preserve snapshot cutoffs and integrity
  checks and do not advance application durable consumers. Reader credentials now
  also require event-stream-scoped ephemeral consumer create/pull/delete permissions;
  no stream-policy update is required. See [ADR 0042](docs/adr/0042-batched-aggregate-history-replay.md).
- **Breaking (command acceptance):** successful event-free commands now persist a
  context-scoped acceptance receipt by default. `CommandReceipt::AcceptedNoEvents`
  replaces transient `NoEvents`; exact retries return `ExactReplay` with no events.
  Empty/read-only transactions are supported by memory and NATS stores, with atomic
  read guards and normal fingerprint/provenance conflicts. Existing schema-2
  event-free receipts remain readable; new split execution uses permanent
  manifest-bearing receipts and separate result state.
  Past transient no-op acceptances cannot be reconstructed retroactively. See
  [ADR 0039](docs/adr/0039-durable-event-free-acceptance.md).
- **NATS read policy:** ordinary aggregate/session reads and stream discovery now
  trust committed writes after local history checks, instead of rereading every
  historical transaction receipt. Use `NatsEventStore::audit_history` or
  `audit_streams` for deep historical receipt, participant, and guard verification.
  Missing or inconsistent historical receipts inserted outside the supported
  store API are no longer detected by every ordinary load. Atomic writes,
  operation-specific replay/reconciliation, and stored wire formats are preserved.
- Website and Studio share the Reference '15 component library, theme behavior,
  responsive layouts and Rostfrei branding.

### Fixed

- Completed replay rechecks current authorization, immutable identity and detail
  availability before exposing a result, including after publication and response
  retrieval. Retained transport responses cannot bypass expired result authority.
- Custom codec/rejection meanings and configured execution policies survive paired
  Test resets. Reset and teardown fence retained transports and stale store handles.
- Typed technical failures retain their source, recovery classification and original
  command evidence. Quarantine remains inspection-first; recovery/redrive is explicit.
- Rustls is locked to patched 0.23.45; command-fixture authentication and native
  restart checks run in the real-NATS CI suite. Subprocess output draining observes
  child exit and retains credential redaction.
- Website overlay checks wait for actual deferred focus restoration within the
  existing deadline, retaining the original accessibility assertion.
- New projects build outside the repository with unmodified manifests by pinning
  Rostfrei dependencies to the available `v0.0.5-alpha` Git release commit.
  The starter pin is independent of CLI version bumps and advances after release
  publication. Names `model` and `tests` now fail before project creation with a
  suggested alternative. CI checks and tests an external, unpatched starter.
- Direct command execution now defaults missing correlation to the operation ID,
  using the same metadata rule as `CommandBus`. Explicit correlation and causation
  are preserved, transaction receipts and events agree across retries, and mapped
  integration events can be published from directly executed commands without
  caller-supplied correlation.
- Bike-rental command consumer and durable names now freeze the existing `--v2`
  identities independently of payload schema versions, preserving durable progress
  and pending work across schema changes. Documentation treats subscription names
  as stable identities and distinguishes command cutover from event replay.
  Startup does not delete or migrate conflicting durables or queued payloads.

### Release scope

- Install Rust crates/CLI from the Git release; the release workflow publishes the
  Agent Skills archive and checksum, not crates.io packages.
- The generated starter still pins published `v0.0.5-alpha`, which predates Skills.
  Upgrade its dependency deliberately before installing 0.1 skills; advancing the
  default starter pin is a [post-publication follow-up](docs/starter-skills-followup.md).
- Aggregate snapshots, whole-history completed-result replay optimization and richer
  integration-event redrive UX remain deferred. Local broker qualification does not
  establish replicated failover, Kubernetes lifecycle or restore qualification.

## [0.0.5-alpha] - 2026-09-16

### Added

- Tracer Studio isolated Test-bus execution, with
  catalog-driven forms, draggable Command/Tests/Runs panels, exact payload
  encoding, and stable idempotency keys for ambiguous submissions.
- Scoped quarantine readers and Tracer list/detail APIs for Test and production.
  Production inspection has a separate read-only credential, default payload
  redaction, and an independently deployable bike-rental inspection host.
- Managed NATS token, credential-callback, signed-challenge, and TLS configuration,
  including custom trust roots and client certificates. Connection deadlines,
  reconnects, health tracking, and graceful drain remain framework-owned.
- First-class NATS username/password authentication through
  `NatsConnectionConfig::with_user_and_password` or percent-encoded URL
  credentials. Authentication is retained across reconnects and server
  failover, including discovered servers. Incomplete or conflicting pool
  credentials are rejected, and credentials are stripped from client addresses
  and excluded from configuration debug output and returned errors.

### Changed

- Attempt-scoped append sessions reuse validated event histories during command
  persistence and verify newly published events without rereading historical
  prefixes. Conflict retries use fresh sessions and retain transaction and
  idempotency checks.
- **Breaking (NATS provisioning behavior):** `provision_event_store()` now creates
  an absent stream or verifies an existing one without updating its policy.
  Move deliberate capacity changes and legacy aggregate-only subject migrations
  to `update_event_store()`. Normal startup continues to use the verification-only
  `NatsEventStore::connect()`.
- **Connection configuration:** username/password URL credentials must be complete
  and consistent across the server pool and match any explicit username/password
  configuration. They cannot be mixed with token or callback authentication.
- **Tracer discovery:** Catalog discovery now advertises `catalogVersion: 1`
  consistently. Consumers of the previous release's `catalogVersion: 2` must
  accept Catalog v1. Behavioral-test documents continue to use schema version 2.
- **Bike-rental example:** fixtures now use explicit `bicycle-added` events.
  `AddBicycle` requires a bicycle identity and condition and rejects duplicates.
  Incompatible earlier example histories require a fresh application namespace.
- Framework crates share workspace-managed dependency versions. The bike-rental
  example is explicitly non-publishable.

### Fixed

- The packaged `rostfrei-structure` CLI includes its scaffolding templates, so
  the package archive compiles and can generate new projects outside the repository.
- Aggregate discovery handles trailing transaction receipts and guards, validates
  transaction provenance and predecessor expectations, and applies one snapshot
  cutoff to both discovered histories and related transaction participants.
- Concurrent event-store provisioning verifies the winning stream without
  mutating its policy, including when creation reports an account-capacity error.
- Unsafe durable-consumer settings are rejected before workers can lose payloads
  or durable progress.
- Studio input refresh preserves edited values and exact numeric selections;
  expired selections require a new choice. Result popups remain within desktop
  and mobile viewports, and graph captions use the actual capture fidelity.

### Development

- A disposable, pinned NATS test runner and GitHub Actions now exercise the Rust
  workspace, broker integration, authentication/TLS, and Studio browser checks.
  Shared-broker tests require explicit configuration instead of silently skipping.
- Repeatable browser inspection captures screenshots, layout state, accessibility
  information, and browser diagnostics.
- Version consistency checks, exact release-tag validation, and an offline
  version-bump command keep manifests and both Cargo lockfiles synchronized.

## [0.0.4-alpha] - 2026-09-08

### Changed

- **Breaking:** commands are now bounded-context operations. Command payloads
  carry every required aggregate ID, and standard HTTP command requests use
  `POST /contexts/{context}/commands/{command}/schemas/{schema_version}` without
  aggregate routing segments.
- **Breaking (Rust API):** `CommandProcessor::new` now takes its
  `BoundedContextName` before the store. Manual `Aggregate` implementations must
  define `BOUNDED_CONTEXT`; the aggregate derive macro generates it from the
  declared domain descriptor. `CommandExecutor` no longer has one global codec
  type parameter: construct it with `CommandExecutor::new(store)` and register
  overrides per aggregate with `.with_codec::<Aggregate, _>(codec)`. For an
  explicit one-off codec, use `rehydrate_with_codec`; otherwise `rehydrate` uses
  the aggregate's registered codec or the JSON default.
- **Breaking (event-store API):** implementations must provide
  `load_transaction_receipt_in_context`. Command transactions and replay use
  `(BoundedContextName, OperationId)` identity; the operation-only method remains
  only for legacy and low-level store tooling.
- **Breaking (NATS observer API):** the aggregate-event subject filter is now a
  required final argument to `NatsCorrelationObserver::new` and
  `new_in_scope`; replace the optional `.with_domain_event_subject_filter(filter)`
  builder call with `NatsCorrelationObserver::new(client, application, filter)`
  or `new_in_scope(client, application, scope, filter)`. Broad domain filters are
  rejected so transaction receipts and guards cannot be observed as events.
- **Breaking:** Tracer schema v2 is the only executable behavioral-test and
  message-series contract. The checked-in v1 JSON Schemas remain unchanged only
  as offline migration references: Tracer does not serve v1 schemas, repositories
  reject v1 behavioral definitions, and removed aggregate-addressed Tracer
  command routes have no compatibility aliases and return `404 Not Found`.
  Migrate behavioral-test documents by changing their document `schemaVersion`
  to `2`. In behavioral tests and standalone message-series definitions, replace
  each command node's `aggregate` object with `context`, move aggregate IDs into
  the command payload, and select the registered command payload schema version.
  Standalone message-series documents have no top-level `schemaVersion`; regenerate
  stored observed series and rediscover all runtime links from Catalog v1 before
  deployment.

## [0.0.3-alpha] - 2026-09-07

### Changed

- **Breaking:** registered HTTP queries now use `POST` with an
  `application/json` body at the existing
  `/contexts/{context}/queries/{query}/schemas/{schema_version}` route. GET
  query routes and query-string payload decoding have been removed.
- Registered queries remain safe and idempotent by application contract despite
  using POST. Command endpoints are unchanged.
