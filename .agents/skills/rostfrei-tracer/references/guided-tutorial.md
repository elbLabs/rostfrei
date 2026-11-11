# Guided tutorial

Use this workflow when a learner asks to explore Rostfrei with an agent. Read
[runtime prerequisites](runtime.md) and apply the token, link-safety, execution
and evidence rules in [the Tracer skill](../SKILL.md). This reference travels
with the existing Tracer skill and its release metadata.

## Teach one step at a time

1. Say what the next step will demonstrate in one or two sentences.
2. Perform that step in the agreed isolated scope, or inspect the selected saved
   evidence. Clearly label an expectation before execution and an observation after it.
3. Explain the result in business language, with the relevant evidence identity
   and any capture limitation. Introduce a technical term only when it helps.
4. Offer a small choice and **pause for the learner**: inspect the command,
   outcome, a particular event or the source rule; continue; or stop.

Do not run the whole tutorial in one batch. Answer follow-up questions against
the same evidence. “Show that again”, choosing another message, changing the
display, or asking why something happened never authorizes a reset or rerun.
Remember the selected operation/message and each evidence file across turns.
If interrupted, reopen those files before considering new execution.

## Establish the environment and scope

For the framework example, follow `docs/agent-tutorial.md` in the selected
checkout for prerequisites and host startup. Use the checkout's pinned Rust,
`cargo rostfrei-dev skills info`, and source CLI throughout. In an installed
application, read this skill's `release.json`, match its locked dependency, and
use `cargo rostfrei` with `--release` as described in the runtime reference.
Never borrow newer online skill instructions to claim an older kit has them.

Record the owned broker/application namespace,
host address, selected definitions and fixture, evidence directory, and owned
processes for cleanup. Keep credentials only in the process environment. Explain
once that startup and each behavior run reset the whole configured **Test**
scenario, invalidating retained Test operations, captures and observation windows.
A run ID is not isolation. Use a disposable broker or uniquely owned application
namespace and serialize runs. Reuse the learner's requested isolated-demo scope;
ask for missing ownership/target information rather than repeating approvals.

Fetch `/catalog` with the control token and check its contract version. Follow
`behavioralTest.definitionsHref`, selected `definitionHref` links, and
`testScenario.fixturesHref` / `fixtureHref`. Read the actual fixture events and
command schemas before describing the Given state. If the expected capabilities
are missing, explain the setup mismatch; do not simulate a successful tutorial.

The bike-rental host registers the two definitions below and `demo-fleet`.
External applications supply their own definitions and fixtures: discover those
and agree equivalent scenarios, or use a separate bike-rental checkout. Do not
invent bicycle commands or fixtures in a consuming application.

## 1. Introduce the two behaviors

In the source checkout, read:

- `examples/bike-rental/tests/tracer/rent-available-bicycle.json`
- `examples/bike-rental/tests/tracer/reject-unavailable-bicycle.json`
- `examples/bike-rental/fixtures/demo-fleet.json`

Verify them against the registered resources. Both definitions use JSON v2 and
fixture `demo-fleet`, revision `2` in this source. The fixture applies two
`bicycle-added` domain events to `city-fleet`: serviceable `bike-42`, then
maintenance-required `bike-99`. Fixture application seeds history; it does not
execute rental commands or publish integration events.

Explain these **expectations**, not yet observed results:

| Behavior | Given | When | Then |
| --- | --- | --- | --- |
| `rent-available-bicycle` | `demo-fleet`, serviceable `bike-42` | `bike-rental` / `rent-bicycle` schema 2, payload `{"fleet_id":"city-fleet","bicycle_id":"bike-42"}` | Accepted; `bicycle-rented` v1 followed causally by `bicycle-rental-started` v1 |
| `reject-unavailable-bicycle` | A fresh `demo-fleet`, maintenance-required `bike-99` | Same command/schema, payload `{"fleet_id":"city-fleet","bicycle_id":"bike-99"}` | Rejected with `BICYCLE_UNAVAILABLE` and rejection payload `{"bicycle_id":"bike-99"}` |

The rejection scenario tests maintenance eligibility. Each run resets the
fixture; this is not a second rental of the bike rented in the first scenario.
Offer to inspect the Given state or begin the successful rental, then pause.

## 2. Run and retain the successful rental

Use a new output path in the owned evidence directory. From the framework root:

```sh
cargo rostfrei-dev test examples/bike-rental/tests/tracer/rent-available-bicycle.json \
  --output "$EVIDENCE_DIR/rent-available-bicycle.json"
```

The runner discovers and validates the definition, resolves the registered
fixture, resets Test, runs the real pipeline, and saves the report and finite
capture. Exit 0 means the suite passed and evidence retrieval completed; exit 1
means a reported suite failure with completed retrieval; exit 2 means blocked
or incomplete execution. A received verdict can survive a later capture error.
Never retry an uncertain submission automatically. Keep its file and explain
`runState`, `captureState` and `error` before choosing any further action.

Read the saved JSON using the agent's file tools. Reuse its existing structure:

| Evidence field | Use |
| --- | --- |
| `release`, `cliRelease`, `definition`, `fixture` | Provenance and the actual test inputs |
| `report.status`, `report.comparison` | Suite verdict and expected-versus-observed diagnostics |
| `report.operation` | Recorded operation status and business decision |
| `messageSeries` | Returned finite operation-capture envelope, with operation/correlation identity and mode |
| `messageSeries.messageSeries.messages`, `messageSeries.messageSeries.commandOutcomes` | Canonical observed messages and command outcomes |
| `messageSeries.capture` | Settling, fidelity and limitations of that finite capture |

`captureState: complete` describes successful retrieval, not proof that every
downstream activity finished. Check `capture.settled` and `capture.fidelity` too.
The report's comparison is not a replacement for the finite message series.

Explain the actual accepted decision and observed messages. Call a domain event
a recorded fact within the rental domain; call an integration event its public
notification. Show the chain only where each child's `causationId` exactly
matches a recorded parent's `messageId`. Correlation, array order, timestamps
and expected graph `parentKey` values do not establish observed edges.
Leave missing parents disconnected; describe grouped/partial captures honestly.
Associate command outcomes by their `commandMessageId`; do not invent an outcome
edge or conclude acceptance from an event count alone.

Offer the command payload, decision, domain event, integration event or source
rule for inspection, then pause. The learner can stay here for multiple turns.

## 3. Preserve, then demonstrate rejection

Before continuing, verify the accepted run's evidence file is readable and
contains its report, operation and finite capture. Keep its path and identities.
If capture failed, explain that and let the learner choose whether to proceed
with partial evidence; do not silently discard the opportunity to inspect it.
Tell the learner the next run replaces live Test evidence, while the saved file
remains available. Continue only after their next-step choice.

```sh
cargo rostfrei-dev test examples/bike-rental/tests/tracer/reject-unavailable-bicycle.json \
  --output "$EVIDENCE_DIR/reject-unavailable-bicycle.json"
```

Inspect this new file. A passing rejection test has suite status `passed` while
the operation completes with a **rejected business decision**. Explain the
recorded `BICYCLE_UNAVAILABLE` rejection and its payload. No captured events
alone cannot prove rejection: distinguish the explicit outcome from the
absence of events within this capture. `failed` or `indeterminate` operation
states do not establish a business rejection.

Compare the two saved runs without resubmitting either. If the learner asks
about policy evaluation, inspect the corresponding source rule and label that
explanation as source-based. The message series does not supply fictitious
policy-evaluation logs or query/projection results. An integration event alone
does not prove a read model caught up or an external effect finished.

Offer to inspect the rejection, compare message identities, read the rule, or
finish. Further commands are new execution choices, not inspection shortcuts.

## Presentation adapter contract

**Text is always available and is the default.** The agent reads existing JSON
evidence and explains it in conversation; no renderer command, Node, Studio,
browser or MCP server is needed. Start with the result, a short supported message
flow and one next-step question. Expand payloads and identifiers on request.

A richer display is optional only when explicitly configured or discovered in
the current harness. Read its actual instructions and supported capabilities;
do not infer a visual app from MCP availability or invent tool/CLI names. Record
which of these presentation capabilities it really supports:

| Capability | Read-only contract |
| --- | --- |
| Show operation | Display an existing report/operation and its finite `messageSeries`, retaining scope, outcome and capture limitations. |
| Inspect message | Inspect an existing `messageId` within the selected operation's canonical series, respecting redaction and missing payloads. |
| Selected message | Return the user's selected operation identity and `messageId`; resolve them against that same evidence before explaining. |

These are capability descriptions, not executable commands or a new wire
schema. Pass the existing JSON evidence, report, operation or MessageSeries
resource as supported; use its existing identities for selection. The adapter
does not publish, reset or modify evidence. Do not give it capability tokens.
If selection is unsupported, let the learner name a message in conversation.
If the selected live resource expired, use its saved capture or report the gap.

Switching adapters renders the same saved evidence and preserves the selection;
it never reruns a test. Fall back to text on unavailable/failed rendering. If a
diagram is requested, apply the skill's explicit-causality and escaping rules;
do not require Mermaid rendering to understand the explanation.

## Finish

Summarize the observed acceptance and rejection, cite both evidence paths, and
state any unresolved capture limitations. Offer the next inspection or behavior
choice. On completion, stop owned host/broker processes and remove only owned
disposable broker storage; retain the learner's evidence. Record any owned
namespace left on a reused broker so cleanup is deliberate.
