# Completed outcomes and trusted envelope ingress (#92)

Status: **INCOMPLETE**. Only validated full-envelope ingress is implemented here.
Shared durable rejected outcomes, migration enforcement, and outcome-expiry policy
are not implemented. Do not deploy this as a fix for contradictory rejected replay.
The existing #92 acceptance tests remain RED rather than being weakened.

## Implemented: preserve a retry's complete HTTP envelope

Applications may opt in with
`HttpApiConfig::default().with_trusted_command_envelope_ingress()` to:

```
POST /contexts/{context}/commands/{command}/schemas/{schema_version}/envelope
Content-Type: application/json
Idempotency-Key: <original operation ID>

<original CommandEnvelope<RoutedCommand> JSON bytes>
```

The ordinary payload-only endpoint is unchanged. Full-envelope ingress is disabled
by default (404); it does not silently honor metadata on the ordinary endpoint.
The application **must** authenticate and authorize this route using middleware.
The router provides neither authentication nor authority to impersonate another
actor. Correlation, causation, timestamp, and internal command-provenance flags
are client evidence, never authentication credentials or proof of authorization.
Only expose this capability to callers trusted to preserve original evidence.

The configured application/traffic scope and URL derive the command address;
clients cannot select another application's subject. Command registration,
schema, route, operation header, envelope identity, correlation/causation,
fingerprint, and adapter payload-size checks precede publication or handler
invocation. Exact `application/json` and a single idempotency header are required.
Existing HTTP operation-ID ASCII restrictions still apply. Size-limited raw bytes
are passed through unchanged; no created_at/correlation/causation regeneration
occurs. Invalid evidence is HTTP 400, not a business rejection.

`CommandBus::dispatch_encoded[_observed]` is the supported protocol-independent
ingress API. It validates complete evidence before dispatch and validates the
response identity. Ordinary typed/dynamic dispatch now uses the same checks.
The HTTP socket tests compare the adapter-observed **entire EncodedCommand** to
the original NATS command. A separate accepted-no-event socket control proves
this new path can replay existing acceptance without invoking the handler.

## Required shared-outcome design, NOT yet implemented

Recording rejection as an accepted event-free transaction would be unsound: legacy
receipt readers would report acceptance, and a fresh processor could not recover
the arbitrary rejection value. A transport cache or process mutex is also not a
durable authority. The current generic executor only persists acceptance and
returns an ordinary rejection without guarded persistence. That behavior is still
present; the capacity and deterministic rejection read-guard tests still fail.

A complete implementation needs a versioned core outcome contract:

* A generic rejection codec/mapping API usable with arbitrary handler rejection
  types; encoding must finish before persistence. Mapping/decoding refusal is a
  typed technical failure, not successful business processing.
* Atomic session/store persistence of a completed accepted **or rejected** outcome,
  separate from domain events. Rejection enlists all successfully loaded aggregate
  versions as read guards and commits no speculative events. A guard conflict must
  reconcile an existing winner or re-evaluate with the normal conflict budget.
* One context-scoped operation/evidence winner, with changed evidence rejected
  before invoking the handler. Acceptance-only receipt APIs must never reinterpret
  rejection as acceptance; existing accepted receipt schemas/readers must keep
  their original meaning. New wire/store versions and conformance tests are needed.
* Reconciliation below every adapter, including direct execution against
  `NatsEventStore`; recreating processors and restarting the broker must not lose
  outcomes. Refused outcome writes must not return/ACK completed rejection.

## Identity and retention gaps

Today's execution fingerprint v2 covers context, command, schema, canonical payload,
and command-provenance mode. Correlation and effective causation are checked
separately by acceptance receipts. Operation IDs are scoped to bounded context;
NATS store configuration supplies application/traffic isolation. `created_at` is
preserved envelope evidence but currently excluded from execution identity. Trace
headers are not operation identity. The new ingress preserves all dimensions; it
does not secretly change the acceptance identity policy to get tests green.

The completed-outcome implementation must explicitly settle whether timestamp is
immutable identity or audit-only, and test every dimension against accepted and
rejected outcomes. It must define a durable horizon independently of broker
publication deduplication and response retention. Existing authoritative acceptance
receipts are append-only; there is currently no shared rejected-outcome expiry or
reuse policy. Expiring a transport response is not permission to reevaluate a
completed operation. Delayed replay after response expiry and consistent identity
refusal/reuse after outcome expiry remain unimplemented acceptance gaps.

## Legacy and deployment gaps

Old NATS retained rejection responses may be the only rejection authority. Ignoring
or deleting them while new direct execution runs can recreate #92. Safe import
requires retained original command evidence, not inventing payload/causation from
a response hash. If source evidence or historical rejection has expired, its
absence cannot establish that an operation was never completed.

Migration must therefore reconcile/import visible legacy evidence or fail closed,
and explicitly handle invisible history with a fenced identity epoch/namespace.
A supported cutover must quiesce old writers, verify import/blocked identities,
enforce old-writer exclusion (for example tested broker ACL/credentials), and
prevent rollback binaries from writing against the new authority. Documentation
alone cannot make old binaries consult a ledger. Neither that deployment fence
nor legacy import/blocking is implemented by this change. Arbitrary rolling
old/new workers and rollback are **unsupported and unproven**, not repaired.

## Remaining acceptance work

Shared rejected replay in both transport directions; mixed-adapter concurrency;
changed-evidence conflicts; real rejection persistence refusal; rejection read
guards; generic core/store conformance; legacy cached rejection reconciliation;
durable horizon/expiry; enforced rolling/rollback restrictions; rejected replay
across independent processes and broker restart; and actual lost-response/PubAck
injection remain incomplete. The original native capacity fixture still exhausts
the real event-store sink because no new ledger sink was introduced. It has not
been changed to test an unused resource or converted into a passing known-bug test.
