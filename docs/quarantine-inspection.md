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
requests per Tracer, each with a six-second service deadline including waiting
for an active Test reset. A failed reset does not itself disable inspection:
any remaining evidence can still be read when its stream is available.

The bike-rental example configures a Test reader automatically. Adopters can
attach an `Arc<dyn QuarantineReader>` using
`TracerBuilder::with_test_quarantine_reader`. Construction rejects a normal-scope
reader registered as Test. The reader needs no command transport or live
operation table, so retained failures remain inspectable after a Tracer restart.
