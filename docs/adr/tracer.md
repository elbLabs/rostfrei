# Tracer and behavioral testing

**Show recorded evidence, not inferred certainty. Test through the real runtime.**

People and agents need to distinguish a proposal, a committed decision, and an
observed effect. A persuasive graph or replay must not blur those distinctions.

## What follows

- Discover registered capabilities and execute through the application's command
  bus. Real Test execution uses isolated Test resources. Production dispatch is a
  separate capability; inspection does not grant permission to execute.
- Compare behavioral expectations with actual outcomes and messages. Causation
  establishes edges; correlation, timestamps, and arrival order do not establish
  causation or business completion. A caught-up projection is not proof that a
  flow has finished; no events is not proof that no command decision exists.
- Make capture windows, retention, missing evidence, and truncation visible.
  Reconstructing or presenting recorded history is a read, not a new execution.
  Do not expose evidence outside the caller's scope or payload policy.
- Validate named, revisioned fixtures before replay through the shared engine.
  Deterministic identities support interruption recovery; fixture loading must
  not trigger post-commit external effects. Multi-stream replay is not atomic.
- Keep handler simulation explicitly diagnostic: a decision and uncommitted
  proposals, with no operation, receipt, or observed trace. It does not sandbox
  injected dependencies and cannot substitute for execution evidence.
- Keep behavioral definitions with the application. Humans and agents use the
  same advertised contracts and evidence, with bounded resource ownership.

## Tradeoff

Real tests require an isolated runtime and asynchronous evidence collection.
Inspection must sometimes answer with incomplete evidence. That is more useful
than claiming certainty beyond the recorded scope.

See [host setup](../nats-application-host.md), [handler diagnostics](../handler-simulation.md),
[observation](../continuous-observation.md), and [quarantine](../quarantine-inspection.md).
