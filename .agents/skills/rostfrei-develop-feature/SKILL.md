---
name: rostfrei-develop-feature
license: EUPL-1.2
description: Develops Rostfrei application features with a human teammate through Given/When/Then and test-first implementation. Use when adding or changing application behavior; writes and explains behavior-suite tests executed against real NATS before implementing to green.
compatibility: Requires the release-matched cargo-rostfrei Rust CLI and an isolated real NATS-backed application test runtime. Building the CLI requires Rust 1.98 or newer. Ordinary conversation works without host-specific tools; consumer installation and execution need no Python.
---

# Develop a feature

Develop one behavior at a time with the human teammate. Use the application's
installed Rostfrei version, real implementation and existing test conventions.
The loop is: agree on behavior, write and explain its test, observe red,
implement, observe green, refactor.

## Evolve contracts through deliberate migrations

Before introducing a version or compatibility path, identify the incompatible
change and the actual callers, deliverable messages, or stored history that need
it. Preserve versions for compatible changes; avoid speculative `V2` types or
parallel business implementations. For a real migration, record rollout order and
an observable retirement condition, and test preserved historical meaning.

Follow the shared [schema and subscription guide](https://elblabs.github.io/rostfrei/docs/source/messaging-upgrades.md)
for reader-first rollout, boundary adapters, historical decoders, and durable
identity. Apply it to the application's installed version. Stored domain-event
readers may be needed indefinitely; a payload-schema migration does not by itself
justify a new durable consumer.

## 1. Discover the intended behavior together

Read the request and relevant repository instructions. Briefly inspect the owning
context and nearby tests so questions use the application's language. Keep
business choices with the human teammate.

Explain who needs the change and the observable before/after in a few sentences.
Use the application's domain, commands and registered fixtures; a bundled example
illustrates test syntax, not the feature to build or its business rules.

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

Identify the selected skills' revision/contracts from installed
`references/release.json` and match the application's locked dependency.
`cargo rostfrei skills info` identifies the invoked setup CLI, which can serve
several application versions. In an existing Cargo application, explicit `cargo rostfrei skills
install` selects its resolved Rostfrei dependency; after dependency upgrades use
`cargo rostfrei skills update`. The setup tool caches matching native CLIs, so
its own version need not equal every application's. Commit installed skills and
their `.rostfrei-managed.json` baseline; resolve local-edit conflicts deliberately.
In the framework checkout, use `cargo rostfrei-dev skills info` and
`cargo rostfrei-dev test ...`; Cargo rebuilds the local Rust CLI after revision
changes. Source discovery requires no generated skill files or preparation step.
Installed kits include `references/release.json`; the CLI checks it when running
their bundled example. For an application-owned definition, pass that metadata
explicitly with `--release <installed-skill>/references/release.json`.

Locate the application's behavior suite, fixtures and NATS-backed Tracer runtime.
Read [the behavior-to-JSON guide](references/worked-example.md) and
[execution instructions](references/execution.md). Read the schema through the
live catalog's `behavioralTest.schemaHref`; installed kits also bundle a snapshot
as `assets/behavioral-test-v2.schema.json`. Match the application framework revision
and live schema before authoring canonical **JSON version 2**; YAML is not an
accepted runner encoding.

Feature acceptance runs through the real NATS command pipeline using the
application's handlers, workers and NATS event store in an isolated environment.
Rust unit tests can support implementation; the feature's red/green evidence
comes from the NATS-backed behavior suite. Within the agreed isolated scope,
“run/test/try” means real Test execution. Optional Tracer **Simulate handler** is
for explicit rule diagnostics only: the real handler's decision and uncommitted
event proposals, with dependencies not automatically sandboxed. It is never a
prerequisite, fallback or source of acceptance evidence. Core simulation tests
remain useful alongside the real behavior suite.

Use the release-matched `rostfrei-tracer` skill to discover the runtime's actual
catalog capabilities before choosing how to inspect it. The Rust CLI executes
suites; direct HTTP handles richer inspection. Read operation/capture evidence to
explain a run, and observation list/detail snapshots for activity initiated by
other clients or workers. Inspect observer health and scoped quarantine when
events are missing or delivery fails. Observation is a partial window, not a
business-completion verdict. Do not assume capabilities or infer them from source
scraping. If that skill is unavailable, the execution reference still covers
suite execution; explain inspection gaps rather than inventing endpoints.

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
Summarize them directly in the conversation, with a focused expected-versus-actual
comparison and a link to the retained evidence. A log path or an agent's completion
notice alone is not a walkthrough. Keep the red definition and capture so the
teammate can compare them with green after the next Test reset.
If the request ends at specifying/building the test, stop there. Otherwise
continue once behavior is clear; keep unresolved business questions with the human.

## 4. Implement, verify and refactor

Implement the smallest change that satisfies the test through the real
application. Keep agreed expectations stable. If new information changes intended
behavior, return to the conversation and revise the test first.

Run the same behavior-suite test against real NATS to observe green. Refactor
with that test passing, then run affected tests and repository-required checks.
Use the Tracer workflow above to diagnose failures from actual evidence;
[execution instructions](references/execution.md) also stand alone.
Simulation cannot substitute for this acceptance run.

## 5. Present the walkthrough and invite feedback

Finish with a short walkthrough directly in the conversation:

1. **Behavior:** the agreed Given/When/Then in the application's language.
2. **Test:** its location and how its fixture, command and assertions express that
   behavior; include the exact reproduction command.
3. **Red:** the actual observed outcome/messages and the assertion they failed,
   with a link to the retained run evidence.
4. **Change:** the implementation location and why the change satisfies the rule.
5. **Green:** the same test's observed result and evidence, stating whether its
   definition stayed unchanged. Explain any agreed expectation revisions.

Include affected checks, application/framework revisions, fixture revision,
isolation and cleanup status. State blocked, failed and unexecuted checks separately
from passing checks. Retain non-secret operation/message-series evidence for both
teammates. Put a durable walkthrough beside the project's test evidence when useful
and link it prominently; the human should not need to search logs for the story.

Invite feedback on whether the behavior, test and red-to-green change were clear.
Record actual feedback and unresolved concerns. Approval of the business rule,
technical test success and feedback on the workflow are separate observations;
claim each only when observed. When proposing improvements to this skill, capture
reusable workflow lessons; keep application-specific rules and trial evidence in
the application.
