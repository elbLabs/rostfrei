# ADR 0015: Application-scoped NATS conventions

## Status

Accepted. Extended by ADR 0018 for the derived test traffic scope.

## Decision

Every rostfrei messaging deployment has one validated application name. The
application name is a lowercase kebab-case NATS token and is the first token in
every application subject. A bounded-context name is also a validated lowercase
kebab-case token.

Messaging addresses follow these conventions:

```text
<application>.command.<bounded-context>.<name>
<application>.command-response.<bounded-context>.<opaque-response-digest>
<application>.integration.<bounded-context>.<name>
<application>.query.<bounded-context>.<name>
```

Quarantined messages stay in the same application namespace and preserve their
source kind and business address:

```text
<application>.quarantine.<source-kind>.<bounded-context>.<name>
```

Private domain-event storage includes both scopes in its subject prefix:

```text
<application>.domain.<bounded-context>.aggregate.<opaque-aggregate-digest>
```

The application name deterministically derives four JetStream stream names and
their subject filters. The bounded context deterministically derives one
authoritative domain-event stream:

```text
<APPLICATION>_COMMANDS
  <application>.command.>

<APPLICATION>_COMMAND_RESPONSES
  <application>.command-response.>

<APPLICATION>_INTEGRATION_EVENTS
  <application>.integration.>

<APPLICATION>_QUARANTINE
  <application>.quarantine.>

<APPLICATION>__<BOUNDED_CONTEXT>_DOMAIN_EVENTS
  <application>.domain.<bounded-context>.aggregate.*
  <application>.domain.<bounded-context>.transaction.>
```

Uppercase stream tokens replace kebab-case hyphens with underscores. Subject
names remain lowercase kebab case. Stream names are operator-facing storage
identifiers; subjects remain the routing contract. The application appears in
both because NATS stream names do not namespace subjects. The double underscore
in a domain-event stream unambiguously separates application and bounded-context
tokens after hyphens have been replaced.

rostfrei owns the normal stream topology, subject filters, retention choices,
finite limits, and provisioning configuration. Applications normally provide
only the application name and, when needed, a replica override. Lower-level
constructors remain available for exceptional stream names and capacities, but
they retain the application-scoped subject convention.

Infrastructure provisioning remains an explicit operator action. Runtime
service startup connects and verifies rather than silently creating or changing
streams.

Stored domain-event wire schemas 3 and 4 record the application and bounded
context and verify both during replay and durable dispatch. The decoder retains
schemas 1 through 3 for already persisted domain events, deriving their scope
from the configured event store. Application and bounded-context scope remain
NATS storage-envelope metadata rather than fields on the transport-independent
`RecordedEvent`; a NATS event store and its durable consumers are already bound
to exactly one configured bounded context.

This decision partially supersedes ADR 0001's exclusion of subjects and stream
names from rostfrei, ADR 0005's deployment-owned event-store naming, ADR 0006's
application-owned full addresses, and ADR 0008's application-owned topology
facade. Applications continue to own business message names, schemas, delivery
classification, environment variables, and operator composition.

## Consequences

Two applications can safely share one NATS account because their first subject
tokens and JetStream filters are disjoint. NATS permissions can grant or deny an
entire application through `<application>.>`. A bounded context is explicit in
every business address and authoritative domain-event subject. An authoritative
stream also stores internal transaction guards and receipts, so
aggregate-filtered domain-event consumers observe gaps in its global stream
sequence and reconstruct their filtered progress across them.

Existing domain-event streams configured with the broader
`<application>.domain.<bounded-context>.>` filter fail runtime verification until
an operator updates or recreates them. Streams that contain non-aggregate
messages require an explicit migration rather than an in-place filter change.

The previous kind-first messaging convention is intentionally breaking. It was
unreleased and receives no dual-publish or dual-consume compatibility layer.
Deployments containing data under previous stream names or subjects require an
explicit migration or recreation before adopting this version.
