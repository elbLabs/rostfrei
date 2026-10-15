# Tracer: real execution, observation and explicit handler diagnostics

This focused follow-up is based on #107, host revision
`b4c79060e8238be695e13b45e0b6593e6d2c268c`, on branch
`feat/tracer-real-execution`. It does not modify #107's branch. Pair it with #106's
release-matched canonical skills; the obsolete `.opencode` Preview-default skill
is removed here and replaced by #106's `.agents/skills` kit.

## Contract

- Catalog no longer advertises `simulateHrefTemplate`; the legacy `/simulate` route
  is absent (404). The legacy Rust `submit_simulation` entry point fails with
  `ModeUnavailable("simulate")` without handler invocation, publication or an
  operation record. It never falls back to Test/Dispatch.
- Studio defaults to real isolated Test. Missing Test disables that action, with
  no fallback. **Simulate handler** is a deliberate separate choice for rule
  diagnostics. Its result stays in the Command panel, outside the observed graph
  and behavioral verdict path. Recorded observation does not submit a command.
- Tracer's distinct handler-diagnostic method uses core `CommandExecutor::simulate`
  with the same registered handler/dependencies and configured history. It returns
  the actual handler decision, starting versions and proposed domain events. It
  creates no synthetic command/response identities, operation or MessageSeries.
  Injected dependencies are not automatically sandboxed; a proposal has no
  authoritative acceptance receipt (ADR 0010/0039). Test is feature acceptance.
- Single application registration and shared handler dependency injection are
  retained. The former `ApplicationBindings.preview` field is named `metadata`;
  payload decoding, input options and real `CommandProcessor` execution remain.
- `CommandRequest` is the neutral envelope name; `SimulationRequest` is a legacy
  alias. `OperationMode::Simulate` and historical predicted-result serialization
  remain solely to identify old data as uncommitted. They cannot create a new
  simulation operation. Historical simulation fidelity is never exact.

Catalog remains v1: action availability is discovered by links. Retired routes
use normal 404 handling, without adding a compatibility subsystem. Behavior and
MessageSeries stay v2, fixtures v1. This capability removal is documented as
breaking in the changelog and requires release-matched clients; no release or
starter pin is advanced here.

## Verification

The original blanket removal was refined with user approval to retain useful
handler computation behind a distinct typed interface. Test-first `abc4380`
observed missing discovery, HTTP 404 instead of 200, missing Studio action
selection, and a missing relation in the actual standalone NATS host after its
real Test runs. These are diagnostic-capability reds, not domain-suite mismatches.
The new tests check injected handler behavior, accepted/rejected/zero-event
decisions, pending-event discard, unchanged history/receipts, no operation or
observation contamination, redaction/bounds, typed failures and control scope.

### Supported handler interface

Control catalog relation:
`contexts[].commands[].versions[].simulateHandlerHrefTemplate`.
Follow its value (currently
`/contexts/{context}/commands/{command}/simulate-handler`) with
`POST {"schemaVersion":1,"payload":{...}}`, using the control bearer token.
No idempotent-publication key is needed. Rust callers use
`Tracer::simulate_handler(context, command, CommandRequest)`.

HTTP **200** returns a distinct `HandlerSimulationResult`, for example:

```json
{
  "kind": "handlerSimulation",
  "context": "counter",
  "command": "increment",
  "schemaVersion": 1,
  "committed": false,
  "outcome": { "decision": "accepted" },
  "streams": [{ "aggregateType": "counter/counter", "aggregateId": "one", "baseStreamVersion": 2, "readGuard": false }],
  "proposedEvents": [{ "aggregateType": "counter/counter", "aggregateId": "one", "proposedStreamVersion": 3, "eventType": "incremented", "schemaVersion": 1, "payload": { "value": 9 } }]
}
```

Rejected results have `outcome.decision: rejected`, a typed JSON `rejection`
(subject to the existing payload policy), and empty `proposedEvents`; loaded
stream participants remain. Accepted empty proposals are legitimate. There is no
Location, operation/correlation ID, receipt, command/response identity or capture.
Default policy removes event payloads and redacts rejection/failure details.
Existing per-result payload budgets apply; oversized results return typed
`413 handler-result-too-large`, not a partial accepted result.

The shared host supplies isolated Test history; `with_test_event_store` also
selects that history. A custom builder's supplied `EventHistory` defines scope.
Computation holds the Test reset read gate and honors failed-reset unavailability.
Its bounded concurrency pool is separate from Test/Dispatch operation admission.
A 30-second deadline/caller cancellation aborts unfinished async work; this does
not undo external dependency effects or sandbox synchronous handler code. Do not
automatically retry. Invalid payloads, history/codec failure, panic and timeout
return typed errors, not business rejection. The result does not validate delivery,
atomic persistence/concurrency, workers or downstream integration activity.

### Earlier Preview retirement evidence

Commit `ab05d5b` established red: discovery still advertised simulation, HTTP
returned 202 instead of 404, and the browser found the Preview mode button.
The focused HTTP/discovery checks now pass. Protocol tests were moved from
simulation to transported Test: redaction, admission, bounded payloads, resumable
SSE, terminal cursors, partial finite captures, idempotency and scope isolation
remain covered. The bike-rental HTTP suite now checks real persisted events and
resumable Test traces. The external NATS consumer checks retired HTTP requests
leave persisted history unchanged after real behavioral runs.

Studio lint/build, mocked interaction tests and all ten visual scenarios passed
locally. Before/after artifacts are retained under
`/tmp/rostfrei-harmony-4wflut29/preview-before` and `preview-after`; desktop command,
accepted flow and mobile details were visually inspected. These are explicitly
mocked presentation evidence, not proof of live execution or independent-agent
competence. The combined live qualification and final CI are tracked on the PRs
and in #106's `docs/agent-skills-harmony.md`.
