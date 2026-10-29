# Command runtime

## Layers

| Boundary | Responsibility |
| --- | --- |
| `CommandBus` / `EncodedCommand` | Canonical command identity, route, metadata, and transport selection. |
| `CommandProcessingService` / `CommandProcessor` | Validation, authorization, outcome lookup, execution, and result projection. |
| `CommandExecutor` / `CommandExecution` | Handler evaluation, aggregate tracking, read guards, winner reconciliation, and conflict retries. |
| `ExecutionStore` / `CommandCommitSession` | Atomic events, guards, and permanent decision through one coordinator. |
| Local, HTTP, NATS adapters | Ingress, delivery, and checked result admission. |
| `OutcomeLifecycle` | Explicit result-detail expiry for an exact winning revision. |

Transport and persistence are independent: local or HTTP calls can use NATS storage.
The [architecture decision](adr/command-execution.md) explains these boundaries.

## Processing

Build a policy-enabled processor with `CommandProcessor::builder(context, backend)`
and an explicit `CommandAuthorizer`. Ingress supplies `TrustedCommandContext`;
command payloads and headers do not establish a principal or security scope.

1. Validate the original envelope and route; optionally audit Started.
2. Authorize ReadOutcome and consult the configured authority.
3. Replay a completed winner. Only authoritative Missing permits Execute
   authorization and handler evaluation.
4. Load aggregates through an attempt-scoped session. Track every successful load,
   including absent streams. Acceptance keeps pending events; rejection discards
   them. Both retain read guards.
5. Consume the session to commit guards, events, and the decision atomically.
   On conflict, reconcile a winner before considering a fresh handler attempt.
6. Validate the returned authority, identity, and result binding; project the
   response and optionally audit Finished. Adapters use `ProcessedCommand::admit`
   before returning full results.

`BoundedConflictRetries` defaults to three retries after the first evaluation.
Zero disables reevaluation after conflicts, not initial lookup or winner
reconciliation. Handlers can run more than once and must account for external effects.

## Identity and results

`OutcomeKey` is authority + bounded context + operation ID. Exact retry preserves
canonical command content, correlation/causation, provenance, security scope, and
optional [actor](actor-attribution.md). Current principal identifies the caller;
it does not partition operation identity. First-completion time is audit data.

`CompletedOutcome<T>` contains a permanent marker, winning revision, and
`OutcomeDetails::{Available, Expired, Archived, Unavailable}`. Only Missing allows
evaluation. Missing, corrupt, expired, or unavailable details remain a completed
decision and produce recovery results/errors. Lookup never runs the handler.

Generic rejection-capable execution requires an explicit `RejectionCodec` through
`execute_with_rejection_codec`. Encoding and round-trip validation precede commit.
To share processor results, use `MappedCommandRejectionCodec` with its forward
mapper and reverse decoder, plus the original encoded command's validated metadata.
Retain readers for stored codec versions. Unsupported or malformed results block
recovery instead of rerunning business logic.

## Shared codecs and audit

Configure one `Arc<AggregateCodecs>` for the processor, executor, fixture engine,
Tracer, and private-event dispatcher. Register or replace codecs before sharing;
shared consumers reject local overrides. Generated JSON is the default.

Audit is off by default. Enabled Started/Finished callbacks are awaited with
`audit_timeout` (one second by default). `ServiceReport` reports audit delivery
separately from the command result; audit failure never triggers reevaluation.
Each invocation has a new attempt ID, including replay. Sink retention/redaction
is application-owned; the built-in bounded memory sink is not a durable audit log.

## Native authority

New coordinated execution selects
`NatsEventStoreConfig::with_fresh_split_outcome_epoch(epoch)`, then explicitly
provisions a journal/outcome-state pair. Both creation incarnations and the epoch
bind the authority. Partial provisioning, recreation, policy drift, and stale
handles fail closed. Follow the [cutover runbook](completed-outcome-cutover.md).

- The append-only journal retains events, guards, and compact decision manifests.
- The state stream retains one Active detail record or terminal Expired record per
  candidate subject. It uses file/Limits storage, bounded capacity, DiscardNew,
  max messages per subject 1, and no age expiry, delete, purge, rollup, or direct access.
- Prepare immutable details with expected subject sequence zero; reconcile an
  uncertain acknowledgement against exact bytes/reference. Preparation is not a
  decision. The atomic journal batch selects the winning manifest.
- Every full-result read checks current leader-served lifecycle state and the
  exact winning reference. Transport caches and optional detail readers cannot
  bypass expiry or substitute a different result.
- `with_read_only_outcomes()` permits history and known-winner replay but refuses
  persistence mutations and expiry. Inline legacy compatibility does not enable
  new coordinated execution; policy replay needs configured legacy security scope.

For authorized native delivery, use `command_handler_with_resolver(service,
resolver)` on workers and `with_outcome_service(service, caller)` on senders.
Senders authorize and look up the winner before queueing and again before exposing
a received business result. Workers reconcile retained responses with authority.
Contextless APIs and the host's `TrustedCompatibility` profile are trusted
infrastructure entry points, without per-call policy admission.

## Detail expiry

Enable `with_explicit_detail_expiry(true)` for native maintenance, then call
`expire_details(key, expected_revision)`. CAS replaces Active with Expired.
`OutcomeExpiry` proves withdrawal; `ReclamationStatus::Reclaimed` additionally
requires verification that the old retained sequence is absent. Otherwise
reclamation may be Pending. Repeat the same revision to reconcile uncertainty.

Permanent decisions, actor attribution, history, and integrity receipts survive.
There is no automatic TTL, archive exporter, or orphan collector. Expiry does not
erase backups or already returned copies. A read admitted before concurrent
expiry may finish; lifecycle checks are observations, not administrative locks.

## HTTP and local adapters

`InMemoryMessagingAdapter::with_service` calls the service for every invocation;
`dispatch_as` supplies trusted caller context. It does not cache completion bodies.
`rostfrei_http::context_command_router` accepts original envelope bytes when
`with_trusted_command_envelope_ingress` is enabled. Middleware supplies
`Extension<TrustedCommandContext>`; route/schema and any supplied idempotency key
must match. Recovery needs no current command binding.

After authorization, expired details map to HTTP 410, transient recovery to 503,
and blocked recovery to typed 500. Denied maps to 403; unauthenticated maps to 401
with a configured challenge, otherwise 403. Responses use `private, no-store`.
See [technical failure recovery](command-failures.md) for delivery failures.

## Source entry points

- [Service](../crates/rostfrei/src/command_service.rs) and [processor](../crates/rostfrei/src/command_bus/processing.rs)
- [Executor](../crates/rostfrei-core/src/executor.rs) and [storage ports](../crates/rostfrei-core/src/execution_store.rs)
- [Native outcomes](../crates/rostfrei-nats/src/event_store/outcomes.rs) and [transport](../crates/rostfrei-nats/src/messaging_adapter.rs)
