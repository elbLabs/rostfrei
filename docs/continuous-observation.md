# Continuous application observation

Tracer can retain and stream a rolling window of real application events without
creating an operation or submitting a command. Studio's **Observe** panel discovers
these flows and follows a selected flow as later events arrive. An action can start
in an application UI, an HTTP handler, a bot, or a background worker.

For earlier evidence still stored in NATS, use
[retained correlation inspection](#retained-correlation-inspection). It reads
history on demand, independently of this rolling observation window.

The current adapter captures **domain and integration events**. It does not consume
the application's command work queue, fabricate missing commands, or establish
that an external side effect completed. Correlation metadata must be propagated by
the application. Messages without valid correlation metadata cannot form a flow.

## Application integration

With the reusable `nats-host`, explicitly enable each application traffic scope on
`TracerConfig`. The host owns subscriptions, validated event decoding, source
readiness and observer lifetime:

```rust,ignore
use rostfrei::ApplicationName;
use rostfrei::tracer::{NatsApplication, ObservationFeed, ObservationScope, TracerConfig};

let application = ApplicationName::new("fast-inbox")?;
let test_feed = ObservationFeed::new(application.clone(), ObservationScope::Test);
let production_feed = ObservationFeed::new(application, ObservationScope::Production);

let tracer = TracerConfig::new(listener, http)
    .with_dispatch_runtime(normal_runtime)
    .with_continuous_observation(test_feed)
    .with_continuous_observation(production_feed);
let host = NatsApplication::builder(test_context, connection, bindings)?
    .with_default_test_fixture(fixture)
    .with_tracer(Some(tracer))
    .provision_and_start().await?;
```

The feed application must match its runtime; Production continuous observation
requires the separately attached Normal runtime. Retained inspection can also use
read-only event-store registrations. See [host configuration](nats-application-host.md).

For custom adapters built directly with `TracerBuilder`, its existing
`with_continuous_observation` accepts the same feeds. Connect the normal NATS event observer to
`tracer.correlation_observer(OperationMode::Dispatch)` and the isolated Test
observer to `OperationMode::Test`. Configuring a feed does not start a broker
subscription. Each adapter must validate the envelope and supply real message,
correlation, and causation identities through `observe_domain_event` and
`observe_integration_event`. Existing operation captures continue to receive their
evidence; the continuous feed also accepts previously unknown correlations.

Register all sources before serving HTTP. A source name identifies one observer's
coverage (for example a bounded context), not an individual correlation:

```rust,ignore
let observer = tracer.correlation_observer(OperationMode::Dispatch);
let source = observer.observation_source("commercial-access")?;
let subscription = nats_observer.subscribe().await?;
if let Some(source) = &source {
    source.ready();
}
// Move source into the handler/task and keep it alive until subscription ends.
// Forward availability(false/true) to source.unavailable()/source.ready().
subscription.run(handler).await?;
```

Readiness is reported only after every registered source is ready. No registered
source means unavailable coverage. Dropping a source guard, including task
cancellation, makes that coverage unavailable. A stopped source can be restarted
under the same name. The NATS correlation handler's `availability` callback reports
broker interruption and recovery; an adapter must propagate it to its source guard.

Bike-rental enables the feeds in
[`bike-rental-api.rs`](../examples/bike-rental/src/bin/bike-rental-api.rs) with
`cargo run --locked -p bike-rental -- --tracer`; the reusable framework host owns
the observer wiring.
Its real-NATS integration test publishes outside Tracer, observes a rental and its
integration-triggered return under one correlation, and retrieves the evidence
through the read-only HTTP API.

## Discovery and HTTP

Catalog v1 optionally includes an `observation` array. Each entry contains:

- `application` and `scope` (`test` or `production`);
- `listHref` for the current retained flow summaries;
- `eventsHref` for continuously replaced summary snapshots over SSE.

Follow advertised links. Current routes are:

| Method and resource | Result |
| --- | --- |
| `GET /observation/{scope}` | Current window, source status, retention counters, flow summaries |
| `GET /observation/{scope}/events` | `observation` SSE frames containing replacement window snapshots |
| `GET /observation/{scope}/flows/{id}` | Canonical `messageSeries`, explicit fidelity, and partial/truncation information |

Each summary advertises its `detailHref`. Scope and generation are checked when
displaying a selected flow. Revisions, generations, and counters are strings so
clients do not lose integer precision. An expired/reset flow returns
`410 observation-expired`; an unconfigured scope returns
`501 observation-unavailable`. An unavailable observer returns an explicit
`status: unavailable` with any retained evidence, rather than a healthy empty feed.

The SSE connection sends an initial snapshot and subsequent replacements. Watch
notifications coalesce updates; reconnect receives the current window. There is no
event replay cursor or claim of complete history. Keep-alive frames arrive every
15 seconds, and slow clients do not accumulate an unbounded event queue.

Test discovery/inspection uses the control capability. Production uses the separate
**read-only inspection capability** for catalog, list, detail, and SSE. Dispatch
credentials do not authorize this feed. An inspection-only host can expose
production observation without installing command execution capabilities.

## Studio

Start the configured application host, then Studio:

```sh
VITE_TRACER_TARGET=http://127.0.0.1:1309 \
VITE_TRACER_TOKEN=local-development-token \
VITE_TRACER_INSPECTION_TOKEN=local-inspection-token \
pnpm --dir studio dev
```

The host must have the corresponding control/inspection tokens configured. The
inspection token is optional when observing only Test. As with the existing Studio
control token, Vite credentials are browser configuration, not server-side secrets.

1. Open **Observe** and choose Test or Production.
2. Wait for **Live**, then trigger an action through the application.
3. Filter by event name or correlation ID and select a flow.
4. Inspect events and payloads as the flow grows. Only explicit causation IDs draw
   graph edges; an unobserved command remains absent.
5. **Pause** freezes browser updates. **Resume** reconnects to the current window.

Observe shares the execution workspace's Canvas/Workbench layouts, message cards,
and details inspector. Canvas closes navigation after choosing a flow; Workbench
keeps it alongside the graph on wide screens. Closing navigation does not pause
observation. Use **Executions** to return to the previous command/test view.

Disconnected, unavailable, resetting, paused, and live states are distinct. A
selected flow that is evicted or reset is labelled as a retained snapshot, and
another flow can be selected. Observation does not fall back to synthetic demo
traffic. Browser failures and failed detail reads are displayed explicitly.

## Evidence and retention

- Each scope retains at most **128 flows**, in last-updated order, with at most
  **256 messages and 256 KiB accounted data per flow**. Metadata has additional
  bounded overhead. Test and production have independent windows and stream limits.
  Event names are limited to 256 bytes before they enter retained summaries.
- There are at most 32 named sources and 16 simultaneous SSE subscriptions per
  scope. Disconnect releases its subscription slot.
- Payload policy is applied **before retention**. The default omits payloads.
  Fixed-size fingerprints detect conflicting duplicate identities even when
  payloads are redacted; raw payloads are not kept in this feed.
- Exact duplicates do not increase message counts or change recency. Conflicting
  identities mark the flow conflicted. Capacity losses mark truncation/eviction and
  increment visible counters. Invalid observations increment omitted-message counts.
- Every flow is explicitly **partial**: this is an observation window, not a
  business-completion verdict. Missing parents and conflicting identities produce
  grouped fidelity. Publication order, timestamps, and correlation alone do not
  establish causation.
- Starting the NATS observer uses `DeliverPolicy::New`; historical events are not
  backfilled. Application restart loses the in-memory window. Stream recreation
  follows the NATS observer's existing generation handling.
- A Tracer Test reset invalidates the Test window and suspends collection during
  reset. A failed or cancelled reset reports unavailable until a successful reset.
  Production observation is unaffected. Broker replay after recreation can include
  fixture domain events; those retain their actual fixture correlation identities.

## Retained correlation inspection

Retained inspection reads domain and integration events by correlation directly
from configured bounded-context event stores and the shared application integration
stream. The transport-neutral `CorrelationHistory` contract is implemented by
`NatsCorrelationHistory`. There is no correlation KV bucket, projection worker,
or backfill. An attached Normal dispatch runtime automatically registers its
domain store and the shared integration source. Additional domain sources or an
inspection-only host use `TracerConfig::with_production_event_store`; no projector
opt-in is needed. See [host registration](nats-application-host.md#retained-correlation-inspection).

This reader requires the [fresh event-subject format](messaging/subjects.mdx#fresh-deployments).
It uses native ordered ephemeral consumers with the official `async-nats` batching
and recovery APIs. The pinned `async-nats` 0.50 client has no `batchDirectGet` API,
although the server supports batch direct get, so the adapter uses those native
consumers instead. Reads capture a finite cutoff per source, clean up their
consumers, and never advance application durables. Reader credentials need scoped
stream INFO, replay-consumer CREATE/INFO/MSG.NEXT/DELETE, and inbox permissions; no
application publication permission is required.

A request accepts at most **256 correlation IDs** across **32 sources**, including
the shared integration stream. Defaults bound the entire request to **2048
references** and **32 MiB of examined data**; `with_limits` can tighten these.
Reaching either bound marks every requested chain truncated, including unresolved
empty chains. Each shared reader handle admits four active requests, each querying
up to four sources concurrently. The overall deadline is 15 seconds, with
five-second API/stall timeouts; timeouts fail the read.

The examined-data budget excludes the client's native 500-message prefetch
buffers: `async-nats` 0.50 exposes no ordered-consumer batch-byte tuning. Successful
reads await consumer deletion. Cancellation schedules cleanup even when creation
is still pending; native 30-second inactivity expiry provides a fallback.

### Discovery and access

Fetch `/catalog` with the **production inspection capability**. A configured reader
advertises `correlations.eventsHrefTemplate` and `correlations.anchors`; each anchor
has `aggregateType`, `label`, and `correlationsHrefTemplate`. Follow these templates
and URL-encode identifiers, including `/` in a full aggregate type.

An aggregate opts into anchor discovery with `const ANCHOR: bool = true` in its
[`AggregateDefinition`](domain-macros/aggregate.mdx). The model/catalog's `anchor`
flag identifies a viewpoint for inspection, not an ownership or causation rule.
The anchor ID is an aggregate ID, not an entity ID inside the aggregate. A
correlation touching multiple anchors can appear under each.

| Request with inspection credentials | Result |
| --- | --- |
| `GET /aggregates/{type}/{id}/correlations` | Correlations recorded on the anchor, with retained references from configured sources |
| `GET /correlations/{id}/events` | JSON containing metadata-only retained references for one correlation |

The existing Control/Dispatch `/correlations/{id}/events` SSE contract continues to
serve its operation captures. It does not grant retained production-history access.
An inspection-only host can serve retained JSON reads without command execution.
Responses use `Cache-Control: private, no-store`.

### Payloads and matching

Direct correlation inspection returns metadata only. Anchored inspection loads the
anchor's own domain payloads through `TracePayloadPolicy`, whose default omits
payloads. Other aggregate streams require an explicit `include=context:<name>`,
including other aggregates in the anchor's own context. Repeat `include` for more
contexts. Integration references remain metadata-only.

Filter an anchored view with repeated `match=<context>/<event>.<field>=<value>`
parameters, for example:

```text
GET /aggregates/ordering%2Forder/O1/correlations?include=context:billing&match=billing/invoiced.amount=10&match=ordering/placed.customer.id=C7
```

Matches are ANDed: each must find a matching domain event among the loaded,
policy-filtered payloads in that correlation. Different matches may match different
events. Field paths address nested fields; numeric segments select array elements.
Strings, numbers, and booleans can match; missing or redacted fields, nulls,
objects, arrays, and integration references cannot. Matching another context
requires its `include`; including a context never bypasses payload policy. All
references for a matching correlation remain in the result.

If the retained read includes newer anchor events than the initial history load,
Tracer reloads that history once and applies the same payload policy, keeping
the original references and source cutoffs. A referenced event still missing
after reload fails the request.

One anchored request selects at most **256 correlations**, retaining the most
recently first-seen correlations in the anchor's history. It accepts at most **16
included contexts**, **64 additional aggregate streams**, and **8 match parameters**.
An over-broad expansion is rejected rather than silently returning some payloads.
Anchor selection or any incomplete chain sets top-level `truncated`, even when
payload matches remove that chain. Each correlation also reports whether its
native read was truncated. An older correlation omitted by anchor
selection can still be inspected directly while its evidence is retained.

### Source cutoffs and retained evidence

Both JSON responses carry a `snapshot` describing the sources queried:

```json
{
  "snapshot": {
    "sources": [
      {
        "context": "ordering",
        "kind": "domain",
        "stream": "SHOP__ORDERING_DOMAIN_EVENTS__SPLIT_<epoch-digest>",
        "cutoff_sequence": "42"
      },
      {
        "kind": "integration",
        "stream": "SHOP_INTEGRATION_EVENTS",
        "cutoff_sequence": "105"
      }
    ]
  }
}
```

`cutoff_sequence` is an inclusive broker sequence encoded as a decimal string.
Domain sources identify their context; the shared integration source has no
`context`. Cutoffs are captured independently, not as a global atomic snapshot.
They describe this read, not live source health, projection lag, or business
completion. Retained responses have no index-lag or source-liveness fields; clients
must not synthesize zero lag from a successful read.

Evidence follows [source retention](messaging/subjects.mdx#retention). In particular,
integration retention remains 30 days by default. Expired integration nodes are
absent, even when surviving domain events still name them as causes. They are not
reconstructed from domain history. Expiry is different from read truncation:
`truncated: false` does not promise a complete historical flow. Only explicit
causation establishes an edge; correlation, timestamps, and an exhausted read do
not establish business completion. Concurrent expiry can remove records below a
captured cutoff.

| Condition | HTTP status and code |
| --- | --- |
| Complete direct read has no retained references | `404 not-found` |
| A retained correlation source cannot be read or validated | `503 correlation-read-failed` |
| Anchor or included aggregate history cannot be read | `503 history-read-failed` |
| An anchor/include context has no configured history | `501 history-unavailable` |
| Retained history reader is not configured | `501 correlation-history-unavailable` |

A source failure fails the request rather than returning a healthy empty or partial
result. A truncated direct read may return an empty `refs` array with
`truncated: true`; it cannot establish absence. An existing anchor with no
correlations returns an empty list, while an unknown anchor type or stream returns
404. See the [HTTP contract tests](../crates/rostfrei-tracer/tests/correlations.rs).
