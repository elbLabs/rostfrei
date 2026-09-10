# Rostfrei project review — 10 September 2026

Reviewed baseline: `41655cf9ce979fc403363963d7be08a8edfdd833` (`origin/main`,
0.0.4-alpha). Fix branch: `fix/project-review-high-priority`.

The review found one high-priority configuration-validation defect with payload
loss and durable-progress implications. This PR fixes it and a related medium-priority
availability defect. Four lower-priority follow-ups remain below. No P0 issue was
confirmed.

## Scope and approach

Reviewed event-store transactions and replay, command processing and response
reconciliation, NATS delivery and provisioning, HTTP/Tracer authorization and Test
reset, fixture/test loading, project scaffolding, repository workflows, and Studio
API/storage code. Ran the Rust workspace suite and real NATS integration suites.
Frontend findings are from source inspection; browser interaction, frontend builds,
dependency vulnerability auditing, deployed configuration, and production load
were outside this pass.

Work was done in an isolated worktree. The original checkout's uncommitted Tracer,
Studio, example, and documentation changes were excluded. The existing performance
[PR #48](https://github.com/elbLabs/rostfrei/pull/48) addresses repeated history
loads and was not duplicated here. This is a focused review, not a claim that every
code path is defect-free.

## Findings grouped by impact

| ID | Priority | Area | Finding | Status |
| --- | --- | --- | --- | --- |
| R1 | P1 — high | Delivery integrity | Startup accepts payload-stripping or unsafe durable-state configuration | Fixed |
| R2 | P2 — medium | Delivery availability | Startup accepts incompatible pull limits and retry/replay settings | Fixed |
| R3 | P2 — medium | Project safeguards | No repository PR workflow runs Rust checks or broker integration tests | Follow-up |
| R4 | P2 — medium | Runtime defaults | Default command response waiting has no deadline | Follow-up; configurable today |
| R5 | P2 — medium | Studio reliability | Browser storage failures can interrupt run-result rendering | Follow-up |
| R6 | P3 — low | Observability | Standard HTTP commands discard incoming trace context | Follow-up |

### R1 — Reject unsafe durable consumer settings before delivery

Location: [`consumer.rs`, `verify_consumer`](../../crates/rostfrei-nats/src/consumer.rs).
Both `NatsConsumerFactory::verify_consumer` and the start of `NatsConsumer::run`
previously checked only a subset of the retained consumer configuration.

A consumer created with the expected durable name, subject, ACK policy, ACK wait,
and pending-message limit still passed validation with `headers_only = true`.
NATS then removes its message payload. The adapter rejects that empty delivery,
writes a quarantine record without the original payload, and terminates delivery.
For the default command WorkQueue stream, this can remove valid work without
executing it. A misconfigured infrastructure resource is the prerequisite; this
is not an unauthenticated HTTP exploit.

The same validation gap accepted `memory_storage = true`, a nonzero
`inactive_threshold`, or an explicit replication count different from the
provisioned contract. These settings can respectively remove restart persistence,
automatically delete a durable consumer, or change its resilience. In particular,
losing an integration-event consumer's acknowledgement state can replay previously
handled messages retained in its Limits stream. These consequences follow from
[NATS's documented consumer semantics](https://github.com/nats-io/nats.docs/blob/master/nats-concepts/jetstream/consumers.md);
the regression directly verifies acceptance/rejection of these configurations,
not a cluster-failure simulation.

**Fix:** compare payload and durable-state settings with the provisioning contract
before any pull. Return `InvalidConfiguration` on mismatch. The new offline test
covers payloads and durable progress; the real-server regression checks both
preflight and worker startup. The original headers-only regression failed on the
unfixed implementation and passed after the fix.

### R2 — Validate the pull contract and delivery timing

Location: [`consumer.rs`, `verify_consumer`](../../crates/rostfrei-nats/src/consumer.rs).
The worker requests batches of `config.concurrency()` messages. A retained
consumer with a smaller `max_batch`, restrictive `max_bytes`, or shorter
`max_expires` previously passed verification even though the worker's pulls could
be rejected. Unchecked server backoff, replay, rate, and priority settings could
also diverge from the application's delivery assumptions.

**Fix:** check these settings and alternate subject filters against the generated
configuration. Integration cases cover `max_batch`, `max_bytes`, `max_expires`,
and `backoff`, alongside R1's cases. Server-generated metadata and the default
pull wait queue remain accepted. Operator-controlled pausing is unaffected.

### R3 — Run validation automatically on pull requests

Locations: [`.github/workflows`](../../.github/workflows),
[`.githooks/pre-commit`](../../.githooks/pre-commit), and
[`messaging_integration.rs`, `test_url`](../../crates/rostfrei-nats/tests/messaging_integration.rs).

The repository contains a website deployment workflow and a manually invoked
release workflow, but no PR workflow for the Rust workspace. The optional local
pre-commit hook checks structure and Clippy; it does not run the test suite.
Several NATS tests also return successfully without executing broker assertions
when `ROSTFREI_NATS_URL` is absent; other integration tests are ignored by default.
A green plain `cargo test` therefore does not demonstrate broker coverage.

**Follow-up:** add PR jobs for locked workspace tests, formatting, Clippy, compiled
domain structure, and an isolated NATS service with the URL explicitly configured
and ignored tests included. Also build both frontend packages. Whether branch
protection requires these checks was not inspected.

### R4 — Make response deadlines explicit at application boundaries

Location: [`messaging_adapter.rs`, `NatsMessagingAdapter::new` and
`read_response_with_timeout`](../../crates/rostfrei-nats/src/messaging_adapter.rs).

`response_timeout` defaults to `None`; the response reader loops on timeout and
unavailability. A successfully published command with no functioning worker or
no eventual response can therefore leave `dispatch` pending indefinitely. This
can retain an HTTP request or occupy a Tracer operation slot. Durable publication
must remain distinct from a failed or timed-out response wait.

**Follow-up:** use the existing `with_response_timeout` at application boundaries,
and document or reconsider the unbounded default. Preserve the operation ID so a
caller can retry/reconcile the same command. This review leaves the current
public timeout policy unchanged because applications may deliberately choose an
unbounded wait.

### R5 — Keep run results usable when local storage fails

Location: [`studio/src/App.tsx`, `storeRun`](../../studio/src/App.tsx).

The React state updater directly calls `localStorage.setItem` while retaining
complete graph nodes, including payloads, for up to 16 runs. A count limit does
not bound serialized bytes. Browser quota exhaustion or denied storage access
can throw during the state update and interrupt rendering of a completed run.
This failure mode was identified from source, not reproduced in a browser here.

**Follow-up:** preserve the in-memory result, perform storage as a guarded side
effect, and cap or evict history by serialized size. Treat inability to persist
history as recoverable.

### R6 — Preserve tracing across the HTTP command boundary

Locations: [`rostfrei-http/src/lib.rs`, `submit_command` and `submit_query`](../../crates/rostfrei-http/src/lib.rs),
[`command_bus.rs`, `DynamicCommandRequest`](../../crates/rostfrei/src/command_bus.rs).

Queries validate and carry `traceparent`/`tracestate`; commands ignore those
headers and the dynamic command request currently has no trace-context field.
HTTP command traces consequently stop at this boundary even though lower-level
messaging supports trace context.

**Follow-up:** define the command trace-context contract and carry it through
encoding and worker delivery, with malformed/duplicate-header coverage.

## Validation

- Unfixed implementation: the real-server headers-only regression failed because
  `verify_consumer` returned `Ok(())` for the unsafe consumer.
- Baseline: `cargo test --workspace --all-features --locked` passed, with 627 tests
  reported passed and 19 ignored. Broker-dependent non-ignored tests can also
  skip when no NATS URL is supplied, as described in R3.
- Final workspace: `cargo test --workspace --all-features --locked` passed
  (630 reported passed, 19 ignored; NATS exercised separately below).
- Final broker suite: `ROSTFREI_NATS_URL=nats://127.0.0.1:52887 cargo test --locked
  -p rostfrei-nats -p bike-rental --all-features -- --include-ignored` passed
  (193 reported passed, none ignored), against a dedicated `nats:2.12` container.
  This includes atomic transactions, durable domain-event consumers, messaging,
  command responses, correlation observers, and bike-rental runtime/reset tests.
- `cargo clippy --workspace --all-targets --all-features --locked` passed.
- `cargo rostfrei-dev check --workspace` passed (one configured domain package).
- `cargo fmt --all -- --check` and `git diff --check` passed.

Counts above are test-harness totals, not unique scenarios; some modules are
compiled into multiple test targets. The NATS and workspace runs overlap. One
intermediate broker rerun hit a missing build artifact while other Cargo commands
were using the same target directory; the final sequential broker run passed.

## Compatibility

The change does not alter message formats or public APIs. Existing consumers
whose delivery or persistence settings differ from the generated contract now
fail startup explicitly. Operators must restore the intended settings or plan a
consumer migration that preserves progress. The adapter does not modify or delete
existing consumers. Valid provisioned consumers continue to work.
