# Continuous application observation

Tracer can retain and stream a rolling window of real application events without
creating an operation or submitting a command. Studio's **Observe** panel discovers
these flows and follows a selected flow as later events arrive. An action can start
in an application UI, an HTTP handler, a bot, or a background worker.

The current adapter captures **domain and integration events**. It does not consume
the application's command work queue, fabricate missing commands, or establish
that an external side effect completed. Correlation metadata must be propagated by
the application. Messages without valid correlation metadata cannot form a flow.

## Application integration

Explicitly enable each application traffic scope in the host:

```rust,ignore
use rostfrei_messaging_core::ApplicationName;
use rostfrei_tracer::{ObservationFeed, ObservationScope};

let application = ApplicationName::new("fast-inbox")?;
let test_feed = ObservationFeed::new(application.clone(), ObservationScope::Test);
let production_feed = ObservationFeed::new(application, ObservationScope::Production);

let tracer = builder
    .with_continuous_observation(test_feed)
    .with_continuous_observation(production_feed)
    .build()?;
```

Connect the normal NATS event observer to
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

The bike-rental host includes this wiring in
[`bike-rental-api.rs`](../examples/bike-rental/src/bin/bike-rental-api.rs) and
[`bike_rental_nats/mod.rs`](../examples/bike-rental/src/bike_rental_nats/mod.rs).
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
  reset. A failed reset reports unavailable until a successful reset. Production
  observation is unaffected. Broker replay after recreation can include fixture
  domain events; those retain their actual fixture correlation identities.

Correlation-specific capture sessions for external test runners are tracked in
[#71](https://github.com/elbLabs/rostfrei/issues/71). Continuous observation and the
Studio implementation are tracked in
[#94](https://github.com/elbLabs/rostfrei/issues/94).
