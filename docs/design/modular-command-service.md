# Modular command processing service

This is the processor/local/HTTP/codec integration checkpoint. Native split storage,
native resolver and authorized sender admission are still pending. It does not
declare the full modular runtime or workspace complete. The independent core
review's marker/detail and proposal/receipt binding corrections must be integrated
before native wire restoration is implemented.

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
transport projection uses `OutcomeDetails::available()` and typed boundary errors,
never synthetic business rejections.

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
