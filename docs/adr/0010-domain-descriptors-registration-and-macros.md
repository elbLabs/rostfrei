# ADR 0010: Domain descriptors and automatic generated registration

## Status

Accepted.

## Decision

rostfrei will define an explicit, machine-readable descriptor model for
aggregate types, commands, events, schema versions, aggregate targets,
rejections, handlers, codecs, and inspection views. One runtime registry will
make these descriptors available to command dispatch, tests, documentation,
compatibility checks, and AI tools.

The descriptor and registry contracts are designed and exercised independently
before procedural macros are introduced. A separate macro crate then generates
descriptor values, codecs, erased handler adapters, testing helpers, schemas,
and registrations from typed application code. Generated behavior must be
inspectable and must compile to public framework contracts.

Annotated aggregates and handlers contribute their generated registrations
automatically through a compile-time or link-time registry. Applications do not
list aggregate modules or handlers in a runtime builder. At startup the runtime
collects, sorts, validates, and exposes every linked registration, failing on
duplicate names, ambiguous handlers, or incompatible descriptors.

Manual registration remains an internal and testing escape hatch, not part of
the normal application developer experience. Source-code scraping and runtime
reflection are not authoritative registration mechanisms.

## Implementation staging

The first implemented vertical slice deliberately stops before automatic linked
registration. Domain commands derive their runtime definitions, and registering
an executable control-plane binding inserts its descriptor directly into the
registry. Generated domain modules remain available for applications that need
an explicit grouping, but are not required for command execution. Registration
uses deterministic storage and does not use `inventory`, linker-time collection,
global mutable state, or runtime reflection.

ADR 0014 subsequently absorbed the richer compiled domain model into rostfrei.
Model-backed commands now retain their structural domain descriptor when
registered through `rostfrei-domain-runtime`. The control plane can deserialize
and simulate explicitly bound commands, but the runtime still does not discover
handlers or modules automatically, subscribe to a bus, or enable live dispatch.
Automatic linked registration in the broader decision remains deferred until
its runtime and deployment tradeoffs are addressed explicitly.

## Consequences

Runtime dispatch, developer tooling, documentation, and AI context share one
declared model instead of building inconsistent views of application code.
Macros reduce boilerplate without hiding a second execution model. Descriptor compatibility
becomes part of the public platform contract, and compile-time diagnostics must
identify duplicate names, unsupported schemas, missing aggregate targets, and
ambiguous handlers. Infrastructure selection and provisioning remain separate
from domain registration; automatic handler discovery does not silently choose
an EventStore, command bus, stream policy, or deployment environment.
