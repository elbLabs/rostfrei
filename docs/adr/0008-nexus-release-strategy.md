# ADR 0008: Nexus integration and release strategy

## Status

Accepted; application topology-facade ownership partially superseded by ADR
0015.

## Decision

rostfrei is developed and committed in its own repository. Nexus consumes the
three runtime crates through Git dependencies pinned to one full commit SHA.
An integrating application may retain a thin policy facade for environment
variables, deployment-specific overrides, operator binary composition, and
temporary compatibility re-exports. rostfrei owns normal address conventions,
stream names, and topology defaults under ADR 0015.

Integrating applications must not vendor rostfrei source or duplicate generic
adapters. The first integration proves messaging through existing application
flows and proves event sourcing through rostfrei's framework contract fixtures.
No production aggregate is converted merely to create an integration example.

For the initial single-command, single-integration-event flow, Nexus does not
add an outbox. It ACKs the command only after the integration event receives a
JetStream PubAck and NAKs publication failures. Command redelivery reconstructs
the exact deterministic event and republishes the same event ID. This is
at-least-once delivery, so integration-event consumers deduplicate by event ID.
An outbox remains a later option only if publication must survive independently
of command retention while commands continue to be ACKed.

The release is not pushed and no merge request is opened without explicit
approval. Integrations pin a reviewed commit from the configured rostfrei
remote; no remote URL is invented.

## Consequences

The framework remains independent while Nexus supplies a realistic compatibility
test. Cargo.lock must show the exact Git source and revision before the Nexus
branch is considered releasable.
