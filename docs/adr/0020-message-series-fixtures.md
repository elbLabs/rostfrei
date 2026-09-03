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

A fixture is a named, revisioned `MessageSeries<FixtureMessage>` and is applied
through the shared `MessageSeriesEngine`.

The engine validates the complete causal topology and typed aggregate history
before writing. It replays only domain-event nodes into their aggregate streams.
Command, command-outcome, and integration-event nodes remain in the series as
provenance and are not executed or published during fixture application.

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

## Consequences

- Fixture state and its causal provenance are explicit, serializable, and
  reviewable.
- Hidden fixture commands and application-specific seeding paths are removed.
- Fixture payloads are decoded through registered aggregate event codecs before
  any stream is changed.
- Non-domain provenance is retained without replaying side effects.
- Reset implementations still own physical resource recreation, but receive the
  exact fixture selected by Tracer.
- Applications must register every aggregate type referenced by their fixtures.
