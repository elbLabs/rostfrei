# ADR 0020: MessageSeries fixtures

## Status

Accepted

## Context

Test state was previously recreated by an opaque reset callback. In the
bike-rental example that callback executed a private `ImportDemoFleet` command,
so the advertised fixture name did not identify the messages that established
the scenario. Behavioral setup commands, API fixtures, and NATS reset also used
separate setup paths.

That split makes a fixture impossible to inspect as part of the same causal
model used for expected and observed behavior.

## Decision

A fixture is a named, revisioned `MessageSeries<FixtureDomainEvent>` and is
applied through the shared `MessageSeriesEngine`.

The engine validates the complete domain-event causal topology and typed
aggregate history before writing every event into its aggregate stream. A
fixture cannot contain commands, command outcomes, or integration events.

Fixture domain events carry explicit stream versions. Event-store identities
are derived deterministically from the fixture revision and message contents.
Existing history and the fixture must match for their complete overlap. This
makes interrupted replay safe to retry while allowing normal history to extend
an already-applied fixture. Each domain-event node is one atomic append; a
multi-stream fixture is preflighted but is not globally atomic.

Tracer stores the concrete registered fixtures. Standalone reset selects the
explicit default fixture, while a behavioral test selects its named fixture.
Behavioral setup commands are not a separate execution mechanism; a different
starting state is represented by a different fixture MessageSeries.

API tests, in-memory examples, NATS provisioning, NATS reset, and behavioral
tests apply fixtures through the same engine.

Fixture events establish aggregate state but never trigger post-commit external
side effects. Durable consumers still reconstruct and validate the complete
atomic event transaction before classifying its provenance. Fixture events are
direct one-event appends, and an exact fixture append is acknowledged without
being dispatched to integration-event mappings. Multi-stream application
transactions remain application-originated and are always dispatched in full.

## Consequences

- Fixture state and causal relationships between its domain events are explicit,
  serializable, and reviewable.
- Hidden fixture commands and application-specific seeding paths are removed.
- Fixture payloads are decoded through registered aggregate event codecs before
  any stream is changed.
- Fixture replay cannot execute commands or publish integration events; this is
  enforced by durable-consumer infrastructure rather than application mappers.
- Reset implementations still own physical resource recreation, but receive the
  exact fixture selected by Tracer.
- Applications must register every aggregate type referenced by their fixtures.
