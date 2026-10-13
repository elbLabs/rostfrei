---
name: rostfrei-develop-feature
license: EUPL-1.2
description: Develops Rostfrei application features with a human teammate through Given/When/Then and test-first implementation. Use when adding or changing application behavior; writes and explains behavior-suite tests executed against real NATS before implementing to green.
compatibility: Requires a Rostfrei application repository, its Rust toolchain, and an isolated real NATS-backed behavior-test runtime. Installed release compatibility is recorded in references/release.json. Ordinary conversation works without host-specific tools.
---

# Develop a feature

Develop one behavior at a time with the human teammate. Use the application's
installed Rostfrei version, real implementation and existing test conventions.
The loop is: agree on behavior, write and explain its test, observe red,
implement, observe green, refactor.

## 1. Discover the intended behavior together

Read the request and relevant repository instructions. Briefly inspect the owning
context and nearby tests so questions use the application's language. Keep
business choices with the human teammate.

Ask only the next missing part, one at a time, in this order:

1. **Given:** What situation are we starting from? Who is acting, and what state
   or facts already exist?
2. **When:** What does the person or system do? What input matters?
3. **Then:** What should be observably different afterward? What result should
   the person or another part of the application be able to see?

Use the host's question tool when available; otherwise ask in the conversation.
Wait for the answer before asking the next question. Preserve supplied details;
ask only about missing or ambiguous behavior. A proposed answer is a suggestion
until the user agrees. A fully specified request needs no repeated questionnaire.

Summarize in domain language:

```text
Given <starting facts>
When <action and relevant input>
Then <observable result>
```

Resolve outstanding business decisions before writing assertions. Start with one
useful scenario; add further scenarios as the agreed feature requires them.

## 2. Build the behavior test first

When using this repository's directly discovered source skill, first run
`python3 scripts/package_agent_skills.py --prepare-source` from the framework
checkout root (requires Git and Python 3.11+). Do this before reading generated
resources, and repeat after switching revision or changing framework inputs.
It generates both skills' relative release metadata/licenses and this skill's
schema from the same inputs as packaging, preserving edits to generated files.
The source runner also refreshes them before each execution. Installed release
skills already contain these resources and require no source checkout/bootstrap.

Locate the application's behavior suite, fixtures and NATS-backed Tracer runtime.
Read [the worked JSON v2 example](references/worked-example.md) and
[execution instructions](references/execution.md). An installed skill includes
`references/release.json` and the runtime's generated
`assets/behavioral-test-v2.schema.json`. Match the application framework revision
and live schema before authoring canonical **JSON version 2**; YAML is not an
accepted runner encoding.

Feature acceptance runs through the real NATS command pipeline using the
application's handlers, workers and NATS event store in an isolated environment.
Rust unit tests can support implementation; the feature's red/green evidence
comes from the NATS-backed behavior suite. Preview supplies predicted feedback.

Translate the scenario directly:

- **Given** becomes a registered fixture establishing starting facts in the
  isolated NATS event store.
- **When** becomes the single root command published through the Test pipeline.
- **Then** becomes its expected outcome and causally related committed domain
  events or integration messages, checked against observed execution.

The suite expresses one root command and an expected message graph. If agreed
behavior needs an assertion it cannot express (for example, a browser display or
an arbitrary read-model query), surface that capability gap before claiming
executable acceptance coverage.

Write the test before changing production behavior. Reuse application handlers
and composition, with deterministic external dependencies where appropriate.
Assert the agreed outcome without duplicating the implementation's algorithm.
Expected behavior can be authored before its API or registration exists. Record
a missing API as an initial compile/discovery failure. If minimal declarations
are needed to execute the test, add them after writing it and continue toward a
failing behavioral assertion before implementing the behavior.

## 3. Explain the test and establish actual red

Show the test or a focused excerpt with file and line references. Explain:

| Agreed behavior | Test construction |
| --- | --- |
| Given | Fixture ID/revision and the persisted facts its events establish |
| When | Root command, schema version and payload sent through NATS |
| Then | Expected outcome and causal messages compared with actual execution |

Provide the exact reproduction command and run it. State the isolated broker or
unique application namespace and the complete Test-reset impact. Reuse the
already authorized specific fixture/suite scope for the TDD loop; ask only if
scope/impact is missing or changes. A run/correlation ID does not isolate NATS.

Report the observed failure and why it corresponds to missing behavior.
Distinguish a behavioral mismatch from an unavailable dependency, invalid fixture
or broken runner; resolve environment blockers before treating a run as red.
If the test already passes, inspect whether the behavior already exists or the
test misses the agreed boundary. Explain that finding before deciding with the
teammate what still needs to change.

Make the explanation and actual behavioral red visible before implementation.
If the request ends at specifying/building the test, stop there. Otherwise
continue once behavior is clear; keep unresolved business questions with the human.

## 4. Implement, verify and refactor

Implement the smallest change that satisfies the test through the real
application. Keep agreed expectations stable. If new information changes intended
behavior, return to the conversation and revise the test first.

Run the same behavior-suite test against real NATS to observe green. Refactor
with that test passing, then run affected tests and repository-required checks.
Use the release-matched `rostfrei-tracer` skill for richer inspection when
available; [execution instructions](references/execution.md) also stand alone.
Preview cannot substitute for this acceptance run.

Finish with Given/When/Then, test and implementation locations, commands/results,
application/framework revisions, fixture revision, isolation and cleanup status.
Retain non-secret operation/message-series evidence for both teammates. State
blocked, failed and unexecuted checks separately from passing checks. Do not
claim fresh-session effectiveness or human feedback unless actually observed.
