# Architectural maxims

These ADRs are the project's architectural maxims: lasting choices that guide
implementation and PR review. Each states a rule, why we chose it, what follows,
and the tradeoff we accept. They are constraints to reason with, not slogans or
a catalogue of shipped features.

| Area | Maxim |
| --- | --- |
| [Domain model](domain-model.md) | Model business meaning once, in executable domain code. |
| [Command execution](command-execution.md) | One operation, one durable decision. |
| [Persistence](nats-storage.md) | Preserve authoritative facts; make acceleration replaceable. |
| [Messaging](messaging.md) | Share explicit contracts, not private history. Version for real migrations. |
| [Read models](read-models.md) | A view answers a question; it does not replace its source of truth. |
| [Tracer](tracer.md) | Show recorded evidence, not inferred certainty. Test through the real runtime. |

When reviewing a change, ask which maxim it exercises and whether its tradeoff
still holds. A deliberate change to a principle needs a reason here; an API,
optimization, or fix normally needs an update to its existing [guide](../README.md).
Add a record only for a distinct architectural choice. Keep each to roughly one
page; Git and PRs retain superseded designs, rollout plans, and review history.
