# Tracer real execution and observation

This focused follow-up is based on #107, host revision
`b4c79060e8238be695e13b45e0b6593e6d2c268c`, on branch
`feat/tracer-real-execution`. It does not modify #107's branch. Pair it with #106's
release-matched canonical skills; the obsolete `.opencode` Preview-default skill
is removed here and replaced by #106's `.agents/skills` kit.

## Contract

- Catalog no longer advertises `simulateHrefTemplate`; the HTTP simulation route
  is absent (404). The legacy Rust `submit_simulation` entry point fails with
  `ModeUnavailable("simulate")` without handler invocation, publication or an
  operation record. It never falls back to Test/Dispatch.
- Studio offers real isolated Test only. Commands without Test capability still
  expose metadata/inputs, but submission is disabled. Recorded observation does
  not submit a command.
- Tracer no longer invokes `CommandExecutor::simulate` or manufactures predicted
  commands, responses or events. Core `CommandExecutor::simulate` and
  `SimulationOutcome` and their domain/kernel tests remain supported. Injected
  dependencies are not universally sandboxed; a prediction has no authoritative
  acceptance receipt (ADR 0010/0039).
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
