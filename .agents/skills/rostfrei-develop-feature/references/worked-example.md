# From application behavior to executable JSON v2

## Start with the application's contracts

Choose the feature from the human's request. Discover its owning bounded context,
registered command and payload schema, fixture IDs/revisions and observable
messages through the application's suite and live catalog. Use those contracts
to turn the agreed scenario into a test:

| Agreed behavior | Executable construction |
| --- | --- |
| **Given** the starting facts | A registered fixture whose ordered domain events establish those facts in the isolated Test store. Inspect its contents, not just its name. |
| **When** the actor takes an action | One root command with the application's context, command name, schema version and complete payload. |
| **Then** the agreed result occurs | The root's business outcome and expected causally related domain events or integration messages, with the relevant payload assertions. |

Choose assertions for the behavior, rather than adapting every feature to one
kind of success:

- **State change:** check the agreed decision and committed facts, including the
  identities and changed values that matter.
- **Rule refusal:** check the agreed rejection code and message graph. HTTP success
  alone does not establish business acceptance or refusal.
- **Downstream consequence:** check the expected integration messages and causal
  relationships. Discover the actual contract; do not infer causation from order.

These are assertion patterns, not prescribed business rules. Add relevant boundary
scenarios only after their behavior is clear. Claims of no emitted events/messages
need complete, exact, settled captures to establish absence. If an observable result
is outside the suite's message-graph capabilities, explain the coverage gap rather
than inventing a JSON assertion.

Explain this mapping to the teammate before running the test. After execution,
show the actual expected-versus-observed difference inline. The feature-development
walkthrough is the application's agreed behavior → test → real failure → change →
same-test pass, with evidence links for both runs.

## Optional concrete syntax example: bike rental

The bundled example below demonstrates the JSON shape for one application. Use
it only with the bike-rental example's registrations. In another application,
author a definition from its discovered contracts; these fixture IDs, commands,
payloads and acceptance rules are not defaults or a checklist for all features.

**Given** there are no rental fleets, **When** I add serviceable bicycle
`skills-bike-01` to `city-fleet`, **Then** the command is accepted and commits
`bicycle-added` with those identities and condition.

Read `assets/add-first-bicycle.json` relative to this skill's root. This is a
complete inline definition accepted by the bike-rental application's actual suite.

Execute with `cargo rostfrei test <skill>/assets/add-first-bicycle.json --output
<new-evidence.json>`. In the framework checkout use `cargo rostfrei-dev test`
with `.agents/skills/rostfrei-develop-feature/assets/add-first-bicycle.json`.

| JSON | Meaning |
| --- | --- |
| `setup.fixture: no-rental-fleets` | Registered fixture revision `1`: an empty domain-event history. Reset creates an empty Test event store. |
| Root `kind: command`, `key: add` | The **When**: bounded context `bike-rental`, `add-bicycle` payload schema `2`; payload supplies the fleet and bicycle identities. |
| Root `outcome: accepted` | The **Then** business decision, separate from HTTP or operation success. |
| Event `parentKey: add` | A committed `bicycle-added` event schema `1` causally descended from this command, with the expected payload. |
| `within: 35s`, `settleFor: 1s` | Bounded execution/capture and idle-settling window. These are not sleeps that prove correctness. |

Fixture registration contains this fixture-v1 document (fixture format is
independent of behavior-definition v2):

```json
{"schemaVersion":1,"id":"no-rental-fleets","revision":"1","messages":[]}
```

For non-empty Given state, inspect a registered fixture's actual domain-event
messages and their ordered stream identities, not just its name. Fixtures apply
events; they do not execute commands or publish integration messages.

The root command carries the action **and** its expected decision. There is no
separate invented `when` field. `key`/`parentKey` are authored graph aliases;
observed edges use actual message/causation IDs. Expected objects use subset
matching; scalar values and array lengths/order must match exactly. A missing
event or wrong condition is a behavioral mismatch even when HTTP returns 200.

This behavior already exists in the release example, so this example should
pass. Do not call a deliberate wrong expectation feature-development red. For a
new feature, write its agreed assertion first, explain it, observe its real
behavioral failure, then implement without weakening the expectation.
