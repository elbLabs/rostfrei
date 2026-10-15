# Modular command processing service

This is the processor/local/HTTP/codec integration checkpoint. Native split storage,
native resolver and authorized sender admission are still pending. It does not
declare the full modular runtime or workspace complete. The independent core
review's marker/detail, proposal/receipt and returned-key corrections are now
implemented in core and this service boundary, awaiting independent re-review.
Native wire restoration must reuse those checked boundaries.
Service-review corrections for adapter result admission, shared-executor override
refusal, retryability, audit terminals and recovery-origin classification are also
implemented. Independent re-review and the native/D3 checkpoints remain pending.

## Processing boundary

`CommandProcessor::builder(context, backend)` takes the full application, traffic
scope and bounded context and an `Arc<dyn CommandBackend>`. An explicit
`CommandAuthorizer` is required; trusted applications may install `AllowAllCommands`.
The object-safe `CommandProcessingService` exposes `process` and `lookup` with a
per-call `TrustedCommandContext`. Application ingress establishes that context;
neither the command payload nor HTTP headers establish a principal or security scope.

Each invocation validates the original envelope and complete route, optionally
audits Started, authorizes ReadOutcome, and reads authority. Only authoritative
Missing permits Execute authorization followed by handler evaluation. Completed
replay needs no binding or rejection mapper. Security scope binds the original
operation identity; changing scope cannot create another operation. Principals
identify authorization/audit callers and do not partition operations or aggregates.

`ServiceReport<T>` separates its result, generated attempt ID and audit-delivery
status. `ProcessedCommand` exposes Committed/Replayed origin and the permanent
marker plus typed detail availability. Expired, archived and unavailable details
remain completed operations. Service lookup never evaluates a handler. Full-result
transport projection uses checked `ProcessedCommand::admit` and typed boundary
errors, never synthetic business rejections.

The existing executor remains the sole owner of reevaluation. Its
`execute_completed_with_rejection_codec` preserves marker, availability and origin
from that same consuming-session loop; the generic executor API materializes its
existing result from the same execution. Zero retries still reconciles winners;
the default allows three reevaluations after the initial attempt. Framework
InvalidRequest raised after reads is mapped inside the guarded attempt.

## Effective codec configuration

Build one `Arc<AggregateCodecs>` and supply it to:

- `CommandProcessorBuilder::event_codecs`;
- `CommandExecutor::with_codecs`;
- `MessageSeriesEngine::with_codecs`;
- `TracerBuilder::with_event_codecs`;
- `DomainEventDispatcher::with_codecs`.

Fixture validation delegates to the registry's erased validator, derived from the
same typed codec instance. Original fixture/event bytes and identities remain
unchanged. Shared installations reject local registration overrides. Standalone
configuration retains fallible duplicate-rejecting registration; explicit replacement
happens before sharing. Generated JSON decoding remains the ordinary default.
`CommandExecutor::new(...).with_codec(...)` retains explicit standalone replacement.
`CommandExecutor::with_codecs(...).with_codec(...)` returns `SharedConfiguration`,
even when the selected registry's Arc is uniquely owned. The core review's earlier
fork characterization now asserts this refusal and separately retains its standalone
override and original-registry historical-meaning controls.

Preview executes with the configured codec and remains read-only. Its current JSON
display contract reports typed `InvalidEventPayload` for opaque event output;
this does not change the stored bytes or decoding semantics. Diagnostic text remains
subject to the configured trace payload/redaction policy.

`CommandProcessorBuilder::rejection_codec` supplies the durable mapped
`CommandRejection` codec for business and framework refusals and binding-free replay.
Encoding is validated and decoded before committing. Generic callers that need the
same representation use `MappedCommandRejectionCodec::with_codec` and the same
canonical metadata and trusted security scope.

## Audit

Audit defaults off. Enabled sinks receive awaited Started and Finished callbacks,
each bounded by `audit_timeout` (one second by default). Failed/timed-out callbacks
are reported separately from the command result and never trigger reevaluation.
Internal conflict retries belong to one attempt. New invocations get new attempt
IDs even when replaying the same operation. Route/envelope validation failures do
not reach the policy or sink.
Denied/Unauthenticated authorization outcomes finish as `AuditTerminal::Denied`;
provider Unavailable/InvalidConfiguration finish as `Failed`. Delivery failures
remain independent of those terminals and the command result.

`InMemoryCommandAuditSink::new(maximum_records)` evicts oldest records at capacity.
Zero capacity reports delivery failure. Its explicit `clear` operation affects no
command or lifecycle authority. Records include original command bytes, principal,
scope, command ID, phase and terminal classification. Applications choose retention
and redaction in their sink. There is no durable queue, background audit worker or
crash/cancellation-complete history promise.

## Local and HTTP adapters

`InMemoryMessagingAdapter::with_service` accepts an independent service implementation;
`dispatch_as` supplies caller context. Every invocation calls the service, including
duplicates, and returns that invocation's authoritative response. The adapter retains
publication diagnostics but no cached completion response body. A contextless
dispatch cannot use an installed context-aware service.

Both context-aware adapters validate the original envelope before calling the service
and call the same `ProcessedCommand::admit(caller, command)` before returning a full
result or recording successful local publication. Admission reconstructs canonical
metadata from the original bytes, binds trusted security scope, checks marker context,
operation, original evidence and receipt integrity, validates all response IDs/route/
schema/correlation, and checks response decision against the permanent marker.
An injected intact foreign-operation/scope completion or contradictory mapped response
fails as `CompletedDetailsBlocked`, without another evaluation or successful diagnostics.

This admission check composes with the processing service's authorization and complete
configured-backend key validation before encoded-to-response projection. A response
DTO does not attest storage authority. Native admission must additionally consult
current authoritative winner/lifecycle state and compare the exact full reference;
the local helper alone is not that native authority consultation.

`rostfrei_http::context_command_router` is local full-envelope ingress. Application
middleware installs `Extension<TrustedCommandContext>`. Envelope bytes are passed
unchanged; route/schema and any supplied idempotency header must match. Envelope
ingress requires `with_trusted_command_envelope_ingress`. No registry/binding is
required for recovery of previously completed commands.

After authorization, expired details map to HTTP 410
(`rostfrei.command.completed-details-expired`); transient recovery failure maps to
503; missing/corrupt/forbidden result recovery maps to typed blocked HTTP 500. Denied
maps to 403, unauthenticated to 401 with the configured challenge (403 without one),
authorization-provider outage to 503 and policy misconfiguration to 500. All
responses carry `Cache-Control: private, no-store`.

Both processor and bus `is_retryable` helpers include authorization-provider
Unavailable and `CompletedDetailsUnavailable` (including pending archive recovery).
Denied/Unauthenticated/InvalidConfiguration, expired and blocked results remain
nonretryable. Retrying uses the same operation through authorization and fresh
authority lookup; completed recovery does not permit reexecution.

`EventStoreErrorOrigin::OutcomeRecovery` preserves error origin independently of the
original `EventStoreErrorKind`. Executor outcome lookup/result validation and memory
post-commit detail recovery attach it; the facade uses it for stable typed recovery
classification across initial lookup, concurrent winner and commit-result paths.
Corrupt/forbidden/misconfigured outcome recovery maps to `CompletedDetailsBlocked`.
Ordinary aggregate-history errors and uncertain writes keep their existing technical
diagnostics. Backends must mark post-commit recovery errors with
`EventStoreError::in_outcome_recovery`; they must not mark an uncertain persistence
failure as recovered completion. A failed read after a durable rejected commit is
recovery failure, not rejected-outcome persistence failure.

`CommandProcessor::new`, contextless `process`/`reconcile_completed`, and
`InMemoryMessagingAdapter::new` are the explicit low-level trusted-infrastructure
compatibility profile. They preserve the existing business response wire contract.
Installing a policy through the builder disables those contextless processor
helpers. The existing HTTP `router` remains application-authenticated service
transport; it does not infer per-user identity.

## Lifecycle result

`OutcomeLifecycle::expire_details` now returns `OutcomeExpiry`, containing the exact
winning revision and a separate `ReclamationStatus::{Pending, Reclaimed}`. Success
proves withdrawal. Memory withdraws and reclaims owned bytes atomically and returns
Reclaimed. Native uncertain transition and retained-message verification remain
work for the native checkpoint; no native reclaim guarantee is claimed here.
