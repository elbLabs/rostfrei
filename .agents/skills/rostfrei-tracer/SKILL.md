---
name: rostfrei-tracer
license: Apache-2.0
description: Explore Rostfrei through a guided tutorial, run real isolated tests, and explain recorded operations, observations or quarantine. Also supports explicit handler diagnostics and authorized production dispatch.
compatibility: Requires a release-matched cargo-rostfrei CLI, Tracer HTTP endpoint and capability credentials. Test acceptance needs application handlers, workers and event store on isolated NATS 2.12.1+. Direct HTTP examples use Bash/curl; no Python is required.
---

# Rostfrei Tracer

Discover live capabilities, execute within the agreed scope, and explain what the
recorded evidence establishes. Use text by default; diagrams are optional.
For “what happened?”, inspect existing evidence rather than submitting another command.

## Choose the workflow

Read the linked reference section for the selected task before acting:

| Task | Workflow |
| --- | --- |
| Guided introduction or bike-rental walkthrough | [Guided tutorial](references/guided-tutorial.md): one step at a time, pause for the learner |
| Set up or resolve a release/runtime mismatch | [Setup](references/runtime.md#setup) |
| Discover or call any HTTP capability | [HTTP requests](references/runtime.md#http-requests), then [catalog and inputs](references/runtime.md#catalog-and-inputs) |
| Develop/verify behavior | [Behavior suites](references/runtime.md#behavior-suites), using `cargo rostfrei test` |
| Run/test/try a command in the agreed isolated scope | [Test and reset](references/runtime.md#test-and-reset) |
| Explain an earlier operation | [Operations](references/runtime.md#operations) and its finite capture |
| Inspect/watch activity from other clients or workers | [Observation](references/runtime.md#observation) |
| Investigate quarantined delivery | [Quarantine](references/runtime.md#quarantine) |
| Explain what a handler would reject or propose | [Handler diagnostics](references/runtime.md#handler-diagnostics), only if advertised |
| Publish to production | [Production dispatch](references/runtime.md#production-dispatch) |
| Draw a requested message graph | [Optional diagrams](references/runtime.md#optional-diagrams) from the same evidence |

Tracer exposes **HTTP JSON plus SSE**. The Rust CLI runs behavior suites; use
direct HTTP for other capabilities. There is no implemented MCP server or
`cargo rostfrei observe`, `simulate` or `preview` command.

## Before acting

- Match installed `references/release.json` to the application's locked dependency.
  In the framework checkout use `cargo rostfrei-dev skills info`; source skills
  need no generated metadata. Global `cargo rostfrei skills info` identifies the
  invoked CLI, which may serve several application releases.
- Read **HTTP requests** before any direct authenticated request. Bootstrap at
  `/catalog`, check `contracts.catalog`, and follow advertised same-origin links.
  Missing capabilities are unavailable; never invent endpoints or substitute
  another action. Command schema versions are independent of protocol versions.
- Use the mode's token from the first catalog request: `ROSTFREI_API_TOKEN` for
  Test and handler diagnostics, `ROSTFREI_INSPECTION_TOKEN` for read-only production
  observation/quarantine, `ROSTFREI_DISPATCH_TOKEN` for production dispatch and its
  operations. Never switch tokens after a `403` or expose them in output, files,
  URLs or process arguments.
- For Test, establish the owned disposable broker or unique application namespace,
  fixtures/definitions and reset impact. A suite run resets the **whole configured
  Test scenario**, including retained evidence; a correlation ID is not isolation.
  Serialize runs and save evidence before the next reset. Reuse an agreed TDD scope;
  ask only for missing scope or changed impact. Inspection does not authorize execution.
- Never automatically retry an ambiguous state-changing request. Preserve its
  evidence and report uncertainty. Production dispatch requires confirmation of
  the exact request independently of Test authorization.

## Interpret evidence

- **Test** runs the real isolated pipeline. **Production dispatch** publishes to
  production. **Simulate handler** returns uncommitted proposals, may invoke
  unsandboxed dependencies, and is never an acceptance substitute or fallback.
- A completed operation can contain an accepted or rejected business decision.
  Failed or indeterminate operations do not establish that decision. A passing
  rejection test is still a rejected business command.
- Read the returned finite `messageSeries` and its settling/fidelity metadata.
  Draw causal edges only where a child's `causationId` matches a parent's
  `messageId`; correlation, ordering and timestamps are insufficient. Leave
  missing parents disconnected and label partial or grouped evidence.
- Observation is a partial rolling window, not complete history or proof of
  business completion. Inspect coverage health and scoped quarantine when
  activity is missing; an unhealthy observer is not evidence that nothing happened.
- Keep agreed payload versions. For an actual migration, follow the
  [schema and subscription guide](https://elblabs.github.io/rostfrei/docs/source/messaging-upgrades.md);
  payload changes alone do not justify a new durable consumer.

## Explain the result

Lead with the scope, observed outcome and a short domain-language explanation.
Include useful evidence identities and capture limitations; compare expectations
for behavior tests or when asked. Expand payloads/protocol details only as useful,
and avoid dumping raw JSON. Use the selected workflow's status fields rather than
forcing operation verdicts onto observations or quarantine.

Use ordinary conversation when a host-specific tool is unavailable. Tutorials
follow their staged pauses; changing presentation reuses saved evidence. Add a
Mermaid diagram only when requested or useful, following **Optional diagrams**.
