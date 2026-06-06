# Changelog

All notable changes to this project are documented in this file.

## [Unreleased]

## [0.0.5-alpha] - 2026-09-16

### Added

- Tracer Studio command Preview and isolated Test-bus execution, with
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
