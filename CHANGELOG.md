# Changelog

All notable changes to this project are documented in this file.

## [0.2.0] - 2026-09-04

### Changed

- **Breaking:** registered HTTP queries now use `POST` with an
  `application/json` body at the existing
  `/contexts/{context}/queries/{query}/schemas/{schema_version}` route. GET
  query routes and query-string payload decoding have been removed.
- Registered queries remain safe and idempotent by application contract despite
  using POST.
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
