# Given/When/Then → executable JSON v2

**Given** there are no rental fleets, **When** I add serviceable bicycle
`skills-bike-01` to `city-fleet`, **Then** the command is accepted and commits
`bicycle-added` with those identities and condition.

Read `assets/add-first-bicycle.json` relative to this skill's root. This is a
complete inline definition accepted by the bike-rental application's actual
suite. Use its shape with your application's registrations; these are example
business identities, not inferred application defaults.

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
