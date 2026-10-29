# Migrating to Rostfrei 0.1

Rostfrei 0.1 combines the modular command runtime, application-linked NATS host,
Tracer handler diagnostics, continuous observation, read models and Agent Skills.
It includes Rust API and native-authority changes from `v0.0.5-alpha`.

## Dependencies and tooling

Use Rust 1.98 or newer. Rostfrei 0.1.3 exposes adapters through a single framework
dependency; follow [dependency setup and imports](getting-started.mdx#one-framework-dependency).
Existing separate adapter dependencies remain supported; keep them on the same
release as `rostfrei`.

Resolve and commit the application's new `Cargo.lock`; inspect `cargo tree` or
`cargo metadata --locked` for mixed framework sources. NATS 2.12.15 is the tested
broker version. Native atomic transactions require NATS 2.12.1 or newer.

Install the matching setup CLI separately:

```sh
cargo install --locked --version 0.1.3 rostfrei-structure
cargo rostfrei skills install
# Use skills update for an existing managed installation after the dependency upgrade.
```

Skill setup follows the application's locked dependency, not whichever CLI happens
to be on PATH. Commit both installed skill directories and their managed baseline;
keep the matching native runner cache for source-free execution. See
[Agent Skills setup](agent-skills-install.md) for update/conflict behavior and
checksum verification of the downloadable kit.

The 0.1 CLI's `new` command still generates the published `v0.0.5-alpha` connection
skeleton and reports that Skills are unavailable for that older pin. To use 0.1
in such an application, explicitly upgrade its framework dependency and lockfile,
then install the matching skills. The shared-host bike-rental example is a separate
composition; the generated skeleton does not acquire Tracer simply by upgrading.

## Coordinated execution and completed results

Custom executor backends implement `ExecutionStore`, combining `EventHistory`,
`OutcomeReader`, an explicit authority and a consuming `CommandCommitSession`.
The pre-release `CommandBackend` spelling is now `ExecutionStore`. Do not emulate
the atomic commit by composing independent event and outcome writers.
`EventStore`/`AppendSession` remain explicit low-level compatibility interfaces.

Accepted, event-free and rejected business decisions are durable. Exact retries
recover the original decision; a new business attempt after repair uses a new
operation ID. Disabling conflict retries prevents handler reevaluation after
version conflicts, but retains original lookup and concurrent-winner reconciliation.

`CompletedOutcome<T>` carries a permanent marker and
`OutcomeDetails::{Available, Expired, Archived, Unavailable}`. The old full-result
representation is explicitly `InlineCompletedOutcome`. Update custom adapters and
exhaustive matches for typed outcome-recovery failures. Missing/unavailable details
do not turn a completed operation into a missing command. Explicit detail expiry
retains the winning decision and event-integrity evidence; there is no automatic
TTL, archive exporter or orphan collector.

Generic rejection-capable execution requires an explicit rejection codec. Preserve
readers for stored codec versions. Share one configured `AggregateCodecs` registry
across processing, fixtures, private-event dispatch and diagnostics; configure
replacements before sharing rather than applying local overrides afterward.
See the [command runtime](command-runtime.md).

## Native authority and deployment

New coordinated NATS execution requires a **fresh split-outcome authority**:
`NatsEventStoreConfig::with_fresh_split_outcome_epoch(...)`. The shared host's
`with_fresh_completed_outcome_epoch(...)` selects that split configuration;
the bike-rental example obtains it from `ROSTFREI_COMPLETED_OUTCOME_EPOCH`.

The journal and outcome-state streams form one incarnation-bound authority.
Provision and back up both, retain their metadata and original identity/routing,
and fence old writers using broker credentials/ACLs. A missing or recreated half,
configuration drift or stale handle fails closed. Do not enable a new epoch on old
history merely because transport responses have expired. Route old retries to
their original authority with their original envelopes and IDs.

The [cutover runbook](completed-outcome-cutover.md) covers fresh namespaces,
legacy replay/import and rollback-reader boundaries; the
[native authority](command-runtime.md#native-authority) is the current split-storage
contract. Retained response caches are never completed-result authority.

The current [event-subject format](messaging/subjects.mdx#fresh-deployments) also
requires a fresh application namespace. It has no old-history fallback or subject
migration tool; the outcome cutover/import APIs do not convert event subjects.
Update ACLs and external subscription filters for correlation/causation suffixes.
Retained correlation inspection follows [source retention](messaging/subjects.mdx#retention),
including the unchanged 30-day integration default.

## Authorization and actor attribution

`CommandProcessor::builder` takes the full bounded-context address and an
`Arc<dyn ExecutionStore>`, and requires an explicit authorizer. Ingress supplies
`TrustedCommandContext`; HTTP payloads/capability tokens do not establish a
principal or security scope. Context-aware senders and workers perform authorized
outcome admission on every invocation. Contextless compatibility APIs and the
host's `TrustedCompatibility` profile remain explicitly trusted infrastructure.

`ActorRef` records original attribution, separately from authenticated principal
and authorization scope. Adding, removing or changing it on an existing operation
changes immutable command identity. Upgrade the entire attributed route before
emitting actor-bearing formats; older integration consumers may drop the metadata.
See [actor formats and deployment](actor-attribution.md).

## Host, Test and diagnostics

Enable `rostfrei-tracer/nats-host`, register handlers/codecs once through
`ApplicationBindings`, and compose `NatsApplication` with the intended execution
profile. Configured authorization, retry and codec policies are retained when
Test reset recreates the paired authority and reconnects its workers.

Test suites reset the complete configured isolated Test topology and retained
Test observations before applying their fixture. Normal resources remain separate.
Explicit shutdown awaits the supervised group; stopping a caller does not retract
a command already published to NATS. See [host ownership](nats-application-host.md).

Legacy Tracer **Preview** routes are retired and return 404. Discover optional
`simulateHandlerHrefTemplate` for a real handler decision and proposed, uncommitted
events. That result has no durable acceptance receipt, operation or observed causal
graph and does not replace real Test execution. Injected dependencies are not
automatically sandboxed. Catalog remains v1, behavior/MessageSeries v2 and fixtures
v1; command payload schema versions are independent. Rediscover advertised links.

Tracer-mediated reset gates handler diagnostics. The existing limitation around a
direct failed `NatsRuntime::reset` and Tracer's separate diagnostic-health flag is
documented in [diagnostic limits](handler-simulation.md).

## Timeouts, capacity and recovery

- `NatsMessagingAdapter::new` now has a **30-second response-reading timeout**.
  Override it with `with_response_timeout(Duration)` when needed. The clock begins
  after publication, its observer and optional replay admission; hosts own their
  end-to-end deadline. Timeout means no confirmed outcome, not cancellation,
  rejection or permission to automatically republish. Recover with original
  identity/evidence. See [response waiting](command-response-timeout.md).
- `ConsumerConfig::concurrency()` limits handlers in one worker. Set
  `with_shared_pending_limit(...)` explicitly when multiple workers share a durable.
  The shared budget includes prefetch and grants neither reservations nor fairness;
  its default remains local concurrency. Retain consumer/durable names independently
  of payload schemas and deliberately reconcile durable policy through provisioning.
  See [capacity sizing](consumer-concurrency.md).
- History reads use bounded aggregate-wide subject-filtered batches. Reader ACLs
  need scoped ephemeral consumer create/pull/delete rights. Ordinary reads validate
  local history; use `audit_history`, `audit_streams` or `with_history_auditing(true)`
  for deep historical receipt checks. Neither response-cache expiry nor a successful
  history read proves that new writes are authorized.
- Quarantine provides inspection and evidence. Redrive is explicit and preserves
  original identity; integration-event redrive UX remains deferred. Read-model
  generations, checkpoints and query cutover remain application-owned; follow the
  [read-model guide](read-models/index.mdx) when adopting those runtimes.
