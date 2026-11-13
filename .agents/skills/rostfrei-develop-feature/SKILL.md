---
name: rostfrei-develop-feature
license: Apache-2.0
description: Develop Rostfrei command-driven application behavior with a human teammate through Given/When/Then and real NATS-backed red/green tests. Use when adding or changing that behavior.
compatibility: Requires the release-matched cargo-rostfrei CLI and an isolated real NATS-backed application test runtime. Building the CLI requires Rust 1.98+. No host-specific tools or Python are required.
---

# Develop a feature

Work one behavior at a time: agree on the rule, write and explain its test,
observe red, implement, observe green, refactor. Follow repository instructions
and existing application conventions. Documentation and mechanical changes use
the repository's relevant checks rather than this feature-acceptance loop.

## 1. Agree on behavior

Inspect the owning context and nearby tests. Describe the actor and observable
before/after in the application's language. Ask only about missing or ambiguous
business choices, one question at a time, in Given → When → Then order. Use the
host's question tool if available, otherwise ordinary conversation. A fully
specified request needs no questionnaire; suggestions remain proposals until agreed.

Summarize:

```text
Given <starting facts>
When <action and relevant input>
Then <observable result>
```

Resolve business decisions before writing assertions. Start with one useful
scenario and add others only when the feature requires them.

## 2. Write the behavior test

Read [execution prerequisites](references/execution.md) and the
[behavior-to-JSON guide](references/worked-example.md). Match installed
`references/release.json` to the application's locked dependency; in the framework
checkout use `cargo rostfrei-dev skills info`. The global CLI's `skills info`
identifies that executable, not necessarily the application's selected release.

Discover the application's suite, registered fixtures, command schemas and live
`behavioralTest.schemaHref`. Use `rostfrei-tracer` for runtime discovery and
inspection when available; the execution reference also stands alone. Author
canonical **JSON v2** against those contracts:

- **Given:** registered fixture events establishing the starting facts.
- **When:** one root command through the real NATS Test pipeline.
- **Then:** its expected decision and causally related committed events/messages.

Use the application's handlers, workers and event store, with deterministic
external dependencies where appropriate. Bundled bike-rental examples teach
syntax; they do not supply another application's fixtures or business rules.
Explain coverage gaps, such as browser output or arbitrary read-model assertions,
before claiming the message-graph suite tests them.

Write the test before production behavior. If an API or registration is missing,
record the compile/discovery failure, add only declarations needed to run, then
reach a failing behavioral assertion. Assert the agreed outcome, not a copy of
the implementation's algorithm. Rust unit tests can support implementation;
simulation is neither a fallback nor feature-acceptance evidence.

## 3. Establish and explain red

Show the test or a focused excerpt and explain its fixture, root command and
expected outcome/messages. Give the reproduction command. Record the owned
broker or unique application namespace, fixture/suite scope and whole-Test reset
impact once; reuse that agreed scope for the loop. A correlation ID is not isolation.

Run the test and show the actual expected-versus-observed failure before
implementing. Retain the definition and capture so the next reset cannot erase
the red evidence. Unavailable dependencies, invalid fixtures and runner errors
are blockers, not behavioral red. If it already passes, explain whether the
behavior exists or the test misses the boundary and agree what still needs changing.

## 4. Implement, verify and refactor

Make the smallest application change that satisfies the rule. Keep expectations
stable; discuss changed requirements and revise the test first when needed.
Run the same real NATS suite to green, refactor, then run affected tests and
repository-required checks. Diagnose from recorded evidence rather than rerunning
to explain an earlier operation. If asked only to specify/build a test, stop there.

Before changing a contract version, identify the incompatible change and actual
callers, messages or stored history affected. Preserve versions for compatible
changes. Follow the [schema and subscription guide](https://elblabs.github.io/rostfrei/docs/source/messaging-upgrades.md)
for reader-first rollout, historical meaning and an observable retirement
condition. Avoid speculative V2 types, parallel business implementations and new
durable consumers solely for a payload-schema change.

## 5. Close the loop

Give a concise walkthrough: agreed behavior → test and reproduction command →
actual red → implementation → same-test green. Link both retained evidence files
and note any agreed expectation changes. Include relevant application/framework
and fixture revisions, checks, isolation and cleanup status; distinguish blocked,
failed and unexecuted checks from passes.

Invite feedback on whether the behavior and red-to-green change were clear.
Business agreement, passing tests and human feedback are separate observations.
Keep application-specific evidence in the application; propose reusable workflow
improvements upstream.
