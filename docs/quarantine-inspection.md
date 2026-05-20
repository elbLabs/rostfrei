# Inspecting quarantined messages through Tracer

Start with `GET /catalog`. A configured Test reader advertises
`quarantine.test.application` and `quarantine.test.listHref`. Use the Control
capability for these read-only requests. Follow the returned `detailHref` and
`nextHref` links rather than constructing record IDs or cursors.

## Test API

```text
GET /quarantine/test
GET /quarantine/test?kind=command&context=bike-rental&name=rent-bicycle&limit=25
GET /quarantine/test/{recordId}
```

The optional filters are `kind` (`command`, `command-response`, or
`integration-event`), `context`, and `name`. A page defaults to 25 records with
a maximum of 100. An opaque `cursor` continues the traversal with the same
filters; the returned `nextHref` contains them. Unknown query parameters,
wildcards, and invalid limits are rejected.

Lists contain compact summaries: storage time, source message identity and
address, failure reason and category, source consumer, delivery attempt,
captured correlation ID, truncation flag, and inspection diagnostics. Payloads
and caller metadata appear only on detail responses. `retainedMessages` is a
stream-wide count captured at the start of that request, not a filtered count or
an incident status. `order` is `oldest-first`; `snapshotSequence` is the upper
bound captured for the traversal. Expiry and deletion can still remove records.
Counts and broker sequences are decimal strings to preserve precision in
JavaScript clients.

Detail responses expose full retained Test payloads as base64 and decode complete
JSON when possible. Payload `status` distinguishes `json`,
`binary-or-invalid-json`, `invalid-base64`, `truncated`, and `unavailable`.
`content` distinguishes the `original-message` payload from a bounded
`quarantine-record` prefix when the record itself cannot be decoded. Truncated
payloads are never presented as complete JSON. Missing legacy size, digest,
failure-category, and correlation fields remain absent.

The recorded failure reason and transport correlation ID are evidence. A generic
reason does not establish the root cause; a transport correlation ID is not
proof of a valid envelope or causal relationship. The original message ID can be
`missing-or-invalid` for malformed deliveries. The opaque record ID, not that
message ID, identifies the quarantined delivery.

## Lifecycle and errors

Test reset removes retained quarantine evidence. Old IDs and cursors return
`410` with code `quarantine-reset`; refresh the catalog/list to begin a new
inspection. A record removed by retention or deletion returns `404`
`quarantine-not-found`. Invalid queries, IDs, or cursors return `400`.

A missing reader or unavailable broker returns `503`, exhausted inspection
capacity returns `429`, and a read timeout returns `504`. Responses carry
`Cache-Control: private, no-store`. There are at most eight concurrent inspection
requests per scope, each with a six-second service deadline including waiting
for an active Test reset. A failed reset does not itself disable inspection:
any remaining evidence can still be read when its stream is available.

The bike-rental example configures a Test reader automatically. Adopters can
attach an `Arc<dyn QuarantineReader>` using
`TracerBuilder::with_test_quarantine_reader`. Construction rejects a normal-scope
reader registered as Test. The reader needs no command transport or live
operation table, so retained failures remain inspectable after a Tracer restart.

## Production API and authorization

Production inspection uses its own credential, configured with
`HttpConfig::with_inspection_token`. It must differ from both Control and Dispatch
tokens. Fetch `/catalog` with that credential and follow
`quarantine.production.listHref`. The inspection catalog contains no Test,
Preview, reset, or command-dispatch links. A Control catalog advertises only Test
quarantine. Dispatch credentials do not authorize quarantine inspection.

```text
GET /quarantine/production
GET /quarantine/production/{recordId}
```

The query parameters, payload/diagnostic structure, pagination, and error codes
match Test. Production reads use independently configured readers and capacity;
they do not wait for Test reset. Register readers with
`TracerBuilder::with_production_quarantine_reader`. Readers must match their
traffic scope, and readers installed together must share the same application.

`HttpConfig::inspection_only` installs just the production inspection credential.
The bike-rental example includes an independently deployable inspection host:

```sh
# Set ROSTFREI_NATS_URL and ROSTFREI_INSPECTION_TOKEN in the deployment environment.
cargo run --locked -p bike-rental --bin bike-rental-quarantine
```

It binds to `127.0.0.1:1310` by default; configure `ROSTFREI_INSPECTION_ADDR` and
`ROSTFREI_APPLICATION` as needed. It connects to existing resources without
provisioning streams, applying fixtures, resetting state, or starting workers.
The combined `bike-rental-api` host also enables production inspection when
`ROSTFREI_INSPECTION_TOKEN` is set.

For a normal JetStream API prefix, a read-only NATS user needs publish/request
permissions for exactly:

```text
$JS.API.STREAM.INFO.<APPLICATION>_QUARANTINE
$JS.API.STREAM.MSG.GET.<APPLICATION>_QUARANTINE
```

and subscribe/reply permissions on its client inbox namespace. Match the actual
API prefix when using a JetStream domain. Business publication, consumer
management, stream creation, purge, and deletion permissions are unnecessary.

## Payload policy

`DefaultQuarantinePayloadPolicy` exposes retained Test payloads and metadata. In
production it removes raw base64, decoded JSON, payload digests, caller metadata,
trace context, and free-form failure reasons. Structured failure categories,
source/delivery identities, storage time, correlation ID, size, and truncation
remain available. Payload `status` becomes `redacted`, including for malformed
quarantine-record bytes. This policy is independent of `TracePayloadPolicy`;
enabling development trace payloads does not expose production quarantine data.

Implement `QuarantinePayloadPolicy` and install it with
`TracerBuilder::with_quarantine_payload_policy` for application-specific rules.
The service applies the policy to list reasons and all detail representations
before any protocol adapter receives them. When returning sanitized JSON, begin
with `payload.redacted()` and then set `json` to the sanitized document; this
removes original base64 and digest representations together. The `redacted`
status indicates that the visible JSON is a policy-controlled view.
