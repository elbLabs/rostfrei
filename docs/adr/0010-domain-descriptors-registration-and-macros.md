# ADR 0010: Domain descriptors and automatic generated registration

## Status

Accepted.

## Decision

Rostfrei will define an explicit, machine-readable descriptor model for
aggregate types, commands, events, schema versions, aggregate targets,
rejections, handlers, codecs, and inspection views. One runtime registry will
make these descriptors available to command dispatch, tests, Studio,
documentation, compatibility checks, and AI tools.

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
registration. It defines command metadata and generated domain modules, and
production applications explicitly call `DomainRegistry::register_module` for
each module. Registration uses deterministic storage, validates a complete
module before mutation, and does not use `inventory`, linker-time collection,
global mutable state, or runtime reflection.

ADR 0014 subsequently absorbed the richer compiled domain model and Studio into
Rostfrei. Model-backed commands now retain their structural domain descriptor
when registered through `rostfrei-domain-runtime`. The runtime still does not
deserialize or route commands, erase or invoke handlers, subscribe to a bus, or
discover modules automatically. Automatic linked registration in the broader
decision remains deferred until its runtime and deployment tradeoffs are
addressed explicitly.

## Consequences

Runtime dispatch, UI forms, documentation, and AI context share one declared
model instead of building inconsistent views of application code. Macros reduce
boilerplate without hiding a second execution model. Descriptor compatibility
becomes part of the public platform contract, and compile-time diagnostics must
identify duplicate names, unsupported schemas, missing aggregate targets, and
ambiguous handlers. Infrastructure selection and provisioning remain separate
from domain registration; automatic handler discovery does not silently choose
an EventStore, command bus, stream policy, or deployment environment.
