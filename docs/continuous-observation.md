# Continuous application observation

Tracer can retain and stream a rolling window of real application events without
creating an operation or submitting a command. Studio's **Observe** panel discovers
these flows and follows a selected flow as later events arrive. An action can start
in an application UI, an HTTP handler, a bot, or a background worker.

For earlier evidence still stored in NATS, use
[retained correlation inspection](#retained-correlation-inspection). It reads
history on demand, independently of this rolling observation window.

For identity-bound reads, administrator approvals, and mandatory access auditing,
use [governed Insights](#governed-insights).

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

1. Select the **Observe** tab and choose Test or Production in **Flows**.
2. Wait for **Live**, then trigger an action through the application.
3. Filter by event name or correlation ID and select a flow.
4. Inspect events and payloads as the flow grows. Only explicit causation IDs draw
   graph edges; an unobserved command remains absent.
5. **Pause** freezes browser updates. **Resume** reconnects to the current window.

Observe shares the execution canvas, message cards, and overlay details inspector.
Choosing a flow closes navigation. Reopen **Flows** to filter or pause; closing
navigation does not pause observation. Use the fixed **Commands**, **Tests**, or
**Runs** tabs to return to those views.

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

This reader requires the [fresh subject format](messaging/subjects.mdx#fresh-deployments).
Native ordered ephemeral consumers discover permanent transaction receipts and
Test fixture manifests at a finite cutoff. The reader validates their atomic
coordinates and fetches the referenced domain events through leader-served raw
message reads. Integration events are discovered directly on their own retained
stream. Original event identities, timestamps, aggregate versions, and broker
positions are preserved; receipts themselves are not graph events.

Discovery uses the official `async-nats` batching and recovery APIs. Ordered
consumers can run on lagging replicas: zero pending messages does not prove that
they have reached the captured cutoff. The reader fetches a cutoff witness from
the stream leader and includes its subject in replay, requiring traversal through
that position. Witness reads and overfetch consume the examined-data budget.
If retention removes the witness, only leader evidence that no records remain
at or below the cutoff establishes an empty selection; otherwise the read fails.
Direct Get responses can come from followers and cannot replace this proof.

The reader cleans up ephemeral consumers and never advances application durables.
Credentials need scoped stream INFO/raw-message GET,
replay-consumer CREATE/INFO/MSG.NEXT/DELETE, and inbox permissions; no application
publication permission is required. Historical receipt validation uses the journal's
authority binding and does not require available result details.
Domain journal identities are pinned when attaching the reader. Replacing a journal
invalidates that handle, preventing old anchor payloads from joining new history.

A request accepts at most **256 correlation IDs** across **32 sources**, including
the shared integration stream. Defaults bound the entire request to **2048
references** and **32 MiB of examined data**; `with_limits` can tighten these.
Receipts, manifests, and fetched validation records consume examined-data budget
even when they yield no events. Event-free acceptances and rejections establish
decisions but add no event references or anchor membership.
Reaching either bound marks every requested chain truncated, including unresolved
empty chains. Each shared reader handle admits four active requests, each querying
up to four sources concurrently. The overall deadline is 15 seconds, with
five-second API/stall timeouts; timeouts fail the read.

The examined-data budget excludes the client's native 500-message prefetch
buffers: `async-nats` 0.50 exposes no ordered-consumer batch-byte tuning. Successful
reads await consumer deletion. Cancellation schedules cleanup even when creation
is still pending; native 30-second inactivity expiry provides a fallback.
These limits do not bound all broker-side subject-index traversal: correlation
selectors traverse lifetime operation branches. See the
[storage tradeoff](adr/nats-storage.md#subject-addressed-inspection).

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

An anchored request can combine aggregate type and ID with repeated
`correlation=<id>` parameters (at most 256). The result contains only requested
correlations recorded on that aggregate's own events, expanded to their retained
cross-aggregate flows. Read-only receipt participation does not make an aggregate
an anchor. Selection happens before the default recent-correlation window, so an
explicitly requested older correlation remains selectable. No matching IDs yields
an empty anchored result.

```text
GET /aggregates/ordering%2Forder/O1/correlations?correlation=checkout-1&correlation=refund-1
```

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

Without explicit correlation IDs, one anchored request selects at most **256
correlations**, retaining the most recently first-seen correlations in the anchor's
history. It accepts at most **16 included contexts**, **64 additional aggregate
streams**, and **8 match parameters**.
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
        "stream": "SHOP__ORDERING__JOURNAL",
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
| Invalid correlation ID or more than 256 selection parameters | `400 invalid-correlation-selection` |
| A retained correlation source cannot be read or validated | `503 correlation-read-failed` |
| Anchor or included aggregate history cannot be read | `503 history-read-failed` |
| An anchor/include context has no configured history | `501 history-unavailable` |
| Retained history reader is not configured | `501 correlation-history-unavailable` |

A source failure fails the request rather than returning a healthy empty or partial
result. A truncated direct read may return an empty `refs` array with
`truncated: true`; it cannot establish absence. An existing anchor with no
correlations returns an empty list, while an unknown anchor type or stream returns
404. See the [HTTP contract tests](../crates/rostfrei-tracer/tests/correlations.rs).

## Governed Insights

The optional `rostfrei` **`insights`** feature supplies one authorization service
for Studio, HTTP, and MCP. It supports data-minimization and access-accountability
controls for DSGVO deployments: verified identities, type/field policies,
time-limited approvals, and durable audit entries for approval-free reads too.
The deployment owner chooses the lawful purposes, role assignments, retention
period, source partitions, and backup/deletion procedures.

### Policies and partial disclosure

A policy key is **source + resource kind + type + schema version**. Resource kinds
are `event`, `command`, `aggregate`, `operation`, `readModel`, and `diagnostic`.
Each type has a record rule and JSON Pointer field rules:

- `requestLess`: readable by an authenticated `insights.read` principal.
- `requestOnly`: requires a separate administrator's unexpired approval.
- `deny`: never disclosed, including under an approval.

Unknown types and unclassified fields are denied. A field rule covers its subtree;
more restrictive ancestor or descendant rules win. Selecting an entire container
cannot bypass protection on a child. A record rule also governs identifiers,
existence, and all its fields. An administrator can approve only some fields and
some frozen record references; record-level approval does not override that subset.

For example, `PUT /insights/policies` with a policy administrator's bearer token:

```json
{
  "expectedRevision": 0,
  "policy": {
    "types": [
      {
        "source": "events",
        "resource": {"kind": "event", "type": "tomato", "schemaVersion": 1},
        "record": "requestOnly",
        "fields": {"/weight": "requestOnly", "/producer": "requestOnly"}
      },
      {
        "source": "users",
        "resource": {"kind": "aggregate", "type": "identity/user", "schemaVersion": 1},
        "record": "requestLess",
        "fields": {
          "/age": "requestLess",
          "/name": "requestOnly",
          "/postaddress": "requestOnly"
        }
      }
    ]
  }
}
```

The sources must be explicitly registered by the application. This example makes
every `tomato` record approval-only; fields not classified above remain denied.
Reading a user can immediately return `/age` while withholding `/name` and
`/postaddress`.

`conditions` on a type can only tighten its base policy, for example:

```json
{
  "when": {"path": "/age", "op": "lt", "value": 18},
  "fields": {"/name": "deny"}
}
```

Conditions and query filters support scalar `eq` and numeric `lt`/`gt`.
When a condition's operand can itself be hidden, its restriction is applied
conservatively, so changes in redaction cannot reveal the hidden value.
This includes record-level and overlapping field restrictions, even when a grant
approves another field in that record.
A query filter on a protected field pauses the **whole query** until authorized;
returning matching ages would otherwise reveal whether a hidden name matched.
`partial` describes the policy restriction independently of matching record count.
An empty partial result cannot establish absence.

### Query and approval workflow

Submit a JSON body to `POST /insights/query`:

```json
{
  "selection": {
    "source": "users",
    "resource": {"kind": "aggregate", "type": "identity/user", "schemaVersion": 1},
    "aggregateType": "identity/user",
    "aggregateId": "user-42"
  },
  "fields": ["/age", "/name", "/postaddress"],
  "purpose": "support-case-42",
  "requestMissing": true
}
```

The response contains `records` with `reference` and pointer-keyed `fields`,
`status` (`partial` or `complete`), `auditId`, `policyRevision`, and an optional
`requestId`. `requestMissing: false` reads only currently permitted data.
Denied fields alone do not create approval requests.

An approver reads the request metadata, then submits:

```text
POST /insights/access-requests/<requestId>/decision
```

```json
{"approve": true, "durationSeconds": 300, "fields": ["/name"], "records": null}
```

`records: null` selects all frozen references; an explicit array selects a subset.
`fields: null` selects all requested fields, including filter operands.
`{"approve": false}` rejects the request. Pending requests last 24 hours; grants
last 1–900 seconds. The same issuer/subject cannot approve its own request, even
through another OAuth client.

The requester repeats the original query with `requestId`. A grant binds its
issuer, subject, client, tenant/application/environment, query, purpose, policy
revision, exact fields, and frozen source versions. New records are excluded;
changed or unavailable approved versions require a new request. Changing the query
or using another identity fails. Any policy update revokes existing requests and
grants. Revocation and disclosure admission are ordered by the same native
JetStream expected-sequence publication. Expiry is checked again after durable
acknowledgement, before data is released.

| HTTP endpoint | Required role |
| --- | --- |
| `POST /insights/query` | `insights.read`; also `insights.request` when requesting missing access |
| `POST /insights/access-requests` | Read and request roles; equivalent to `requestMissing: true` |
| `GET /insights/access-requests/{id}` | Request owner, or `insights.approve` in the same scope |
| `GET /insights/access-requests` | `insights.approve` |
| `POST /insights/access-requests/{id}/decision` | `insights.approve` |
| `POST /insights/grants/{id}/revoke` | `insights.approve` |
| `GET`, `PUT /insights/policies` | `insights.policy.manage` |
| `GET /insights/audit?limit=50&before=<cursor>` | `insights.audit.read` |

The request owner's status view omits frozen record counts and identifiers.
An approver receives selection metadata, never the protected field values.
Audit paging returns `nextBefore`; omit `before` for the newest page.

### Keycloak and MCP

Configure a confidential resource-server client, for example `insights-api`,
with the five client roles above. Configure a public Studio client with
Authorization Code flow, **PKCE S256**, exact redirect URIs and web origins.
MCP clients use separately registered OAuth clients. Add an audience mapper for
`insights-api` and include its client roles in access tokens/introspection.
Administrator-controlled mappers must supply string claims `tenant`,
`application`, and `environment`; do not source these authority claims from
user-editable profile attributes.

`OidcAuthenticator::discover` retrieves discovery from the configured issuer and
uses authenticated online access-token introspection on every HTTP/MCP request.
It checks active status, exact issuer, audience, bearer type, expiry/not-before,
allowed client, scope claims, and audience-specific client roles. Introspection
credentials remain server-side. HTTPS is required except for an explicit numeric
loopback HTTP option for local identity-provider testing. Principal expiry is
rechecked after service work. Caller-supplied usernames, actor metadata, request
bodies, and legacy Tracer tokens never establish an Insights identity.

`POST /mcp` provides stateless Streamable HTTP with JSON responses and protocol
`2025-11-25`. Initialize, then send `MCP-Protocol-Version: 2025-11-25`,
`Authorization: Bearer …`, and `Accept: application/json, text/event-stream`.
The tools are `insights.query`, `insights.request_access`, and
`insights.get_access_request`. Query arguments match HTTP; approval administration
uses the HTTP endpoints/Studio. GET streaming and server sessions are not provided.
OAuth resource metadata is served at `/.well-known/oauth-protected-resource`
and `/.well-known/oauth-protected-resource/mcp`; 401 responses advertise it.
Configure the MCP client's Keycloak registration and external resource URL to
match this deployment.

### Host and Studio setup

Use `Insights::new(scope, journal, &binding_key)` with a persistent random
32-byte server secret. Share it across replicas; keep it outside NATS snapshots.
HMAC bindings prevent offline guesses of filter literals and content-hash versions.
Rotating the key invalidates outstanding requests/grants. Record identifiers
remain visible when the record rule allows them; use opaque source identifiers.

Register `EventHistorySource::new(scope, history)` for JSON event payloads or
`AggregateStateSource::<A>::new(scope, history, codec, state_schema_version)` to
rehydrate actual application state. Both require tenant/environment-isolated,
validated history handles. They check the bound scope but cannot establish that
an incorrectly supplied NATS account belongs to a tenant. Custom `InsightsSource`
adapters provide commands, operations, read models, diagnostics, or non-JSON codecs.
They must return stable versions and a complete bounded selection, and must not
consume an application's command work queue to implement inspection.

Serve `insights::http::router(InsightsHttp::new(...))` directly, or pass
`HttpConfig::governed(insights_http)` to the application-linked host. Governed mode
serves the governed routes exclusively. Legacy catalog, observation, correlation,
quarantine, operation, and dispatch routes are absent on that listener. Remove
public access to any separately deployed legacy inspection listener during cutover.

The compiled [governed host example](../crates/rostfrei-tracer/examples/governed-insights.rs)
attaches an existing Normal-traffic JSON event history as source `events`.
Configure these environment variables:

| Configuration | Variables |
| --- | --- |
| Existing isolated source | `NATS_URL`, `INSIGHTS_CONTEXT`, `INSIGHTS_TENANT`, `INSIGHTS_APPLICATION`, `INSIGHTS_ENVIRONMENT` |
| Authority | `INSIGHTS_JOURNAL`, `INSIGHTS_BINDING_KEY_FILE` (32 raw random bytes) |
| Initial provisioning | `INSIGHTS_REPLICAS` (1, 3, or 5), `INSIGHTS_MAX_BYTES` (at least 1 MiB), `INSIGHTS_RETENTION_SECONDS` |
| Identity provider | `INSIGHTS_ISSUER`, `INSIGHTS_AUDIENCE`, `INSIGHTS_INTROSPECTION_CLIENT`, `INSIGHTS_CLIENT_SECRET_FILE`, `INSIGHTS_ALLOWED_CLIENTS` (comma-separated) |
| HTTP | `INSIGHTS_RESOURCE` (external MCP URL), `INSIGHTS_BROWSER_ORIGINS` (exact comma-separated origins), optional `INSIGHTS_LISTEN` (default `127.0.0.1:1309`) |

```sh
# First deployment: explicitly provision a new deny-by-default authority.
cargo run --locked -p rostfrei-tracer --features insights --example governed-insights -- --provision
# Subsequent startup: attach and validate existing authority.
cargo run --locked -p rostfrei-tracer --features insights --example governed-insights
```

For a local numeric-loopback Keycloak issuer, explicitly set
`INSIGHTS_LOOPBACK_HTTP=true`. Install the desired policy through an authenticated
policy administrator after provisioning.

Studio's optional governed workspace uses the official Keycloak JS client:

```sh
VITE_INSIGHTS_ENABLED=true \
VITE_INSIGHTS_ISSUER=https://identity.example/realms/garden \
VITE_INSIGHTS_CLIENT_ID=studio \
VITE_INSIGHTS_AUDIENCE=insights-api \
VITE_INSIGHTS_API_URL=/api \
VITE_TRACER_TARGET=http://127.0.0.1:1309 \
pnpm --dir studio dev
```

Allow the actual Studio origin in the host and Keycloak. Use a same-origin `/api`
reverse proxy in production; the host's origin allowlist does not enable CORS.
Studio keeps tokens, query data, and approval results in memory, refreshes tokens,
and exposes Query, Access requests, Policies, and Access journal according to roles.
Vite settings contain client identifiers and URLs, never introspection secrets.
Governed reads do not enter Studio's persisted run history.

### Audit durability, retention, and recovery

Every disclosure waits for a durable audit acknowledgement. An unavailable/full
journal or uncertain acknowledgement withholds even approval-free data.
Authenticated attempts, denied administration, decisions, policy changes, and
audit reads are journaled. Disclosure entries record server time, issuer/subject/
client, scope, purpose, policy revision, source/type/selection, record versions,
and disclosed/predicate field paths. They omit payloads, bearer tokens, and query
filter literals. Keep purpose codes and identifiers free of unnecessary personal
data. `admitted; delivery unconfirmed` records authorization to release data,
not proof that the client received it or a person consumed it.

`NatsInsightsConfig` requires explicit finite retention and capacity. The
file-backed stream rejects new writes at capacity and denies message deletion
and purge. Native `max_age` expires entries. Each entry atomically carries the
current policy/request snapshot, pruning expired requests from subsequent writes.
Old copies of request metadata can survive until their entry's retention expires;
the maximum is request expiry plus the configured retention window. This snapshot
design deliberately trades write amplification for one consistent authority.

If there are no accesses for an entire retention window, the latest state also
expires. Reads/startup then fail closed; they never recreate permissive state.
Recovery is explicit: stop the old listener, provision a new named authority,
install its reviewed policy, and switch the listener. Requests must be made again.
Apply the same planned rotation when changing retention or retiring a scope.
Delete retired streams and backups according to the deployment's retention rules.
Changing a running stream's retention/capacity or recreating it invalidates pinned
handles. Provisioning is separate from normal startup.

Grant windows are at most 15 minutes; requests hold at most 256 frozen records,
64 fields and 8 predicates. A service admits at most 32 sources and 256 retained
requests. Source/result data is limited to 1 MiB, HTTP bodies to 64 KiB, journal
entries to 512 KiB, and query work to 15 seconds. History adapters load their
bound history before checking its byte size; use suitably bounded source
partitions. Capacity exhaustion is explicit, never silent truncation.

NATS service credentials need publication only to the authority's `.state`
subject, scoped stream INFO/raw GET, and response inbox permissions. Source
credentials need only the relevant history reader's permissions. Provisioners
own stream creation/lifecycle permissions. Keep broker administration and raw
journal access separate from browser/MCP identities.

The [inspection contracts](../crates/rostfrei-tracer/tests/correlations/insights.rs)
run through `cargo xtask test`; their Keycloak/HTTP/MCP journey owns a disposable
Keycloak 26.8.2 container and uses real NATS history. See the
[testing policy](testing/strategy.md) for the complete workspace gate.
