# ADR 0008: Nexus integration and release strategy

## Status

Accepted.

## Decision

Zeitstrahl is developed and committed in its own repository. Nexus consumes the
three runtime crates through Git dependencies pinned to one full commit SHA.
Nexus may retain `nexus-messaging` as a thin policy facade for environment
variables, deployed stream names, topology defaults, an integrating application address policy,
operator binary composition, and temporary compatibility re-exports.

Nexus must not vendor Zeitstrahl source or duplicate generic adapters. The first
integration proves messaging through existing an integrating application flows and proves event
sourcing through Zeitstrahl's framework contract fixtures. No Nexus aggregate is
converted merely to create an integration example.

The release is not pushed and no merge request is opened without explicit
approval. If no Zeitstrahl remote exists, the local release is completed and
Nexus's final pin waits for an operator-provided Git URL; no remote URL is
invented.

## Consequences

The framework remains independent while Nexus supplies a realistic compatibility
test. Cargo.lock must show the exact Git source and revision before the Nexus
branch is considered releasable.
