# Message schemas and subscription generations

Payload schema versions, durable consumer generations, and crate versions are
independent. Subjects do not select a payload schema version.

## Version for a migration

Keep one current command or integration-event contract and one current business
implementation. Schema versions support incompatible contracts that must coexist;
they are not counters for features, refactors, or releases.

Before adding a version, identify the incompatible change and its concrete
compatibility obligation: deployed callers or consumers, queued messages, retries,
retained history, or quarantine redrive. Preserve the version for compatible
changes. When no compatibility obligation exists, update the current contract
directly rather than inventing legacy clients or speculative `V2` handlers.

For a real migration, record the affected callers/messages, rollout order, and
observable retirement condition in the change. Adapt older payloads at the boundary
into the current implementation where their semantics permit. Retire that adapter
when old callers are gone and no pending delivery, retry, retained message, or
quarantine entry can require it.

## Payload schema upgrades

Typed integration decoding accepts the type's exact `SCHEMA_VERSION`; the standard
integration-command handler quarantines unsupported schemas. For a rolling upgrade:

1. Deploy consumers accepting old and new schemas through
   `MessageHandler<IntegrationEventAddress>`, mapping each to the intended command.
2. Switch producers after all affected consumers support the new schema.
3. Retain old readers while pending delivery, retries, retained history, or
   quarantine redrive can still supply old messages.

Changing a durable name does not filter out old schemas. A version bump supplies
no automatic translation; applications own the compatibility mapping.

The application host allows one integration handler per address. Registering v1
and v2 typed handlers side by side is rejected: they would share a durable and
quarantine each other's deliveries. Use one `MessageHandler<IntegrationEventAddress>`
that accepts both schemas during the migration.

## Stored domain events

Historical domain schemas have a different lifetime: retain their readers as long
as stored history needs them, potentially indefinitely. Preserve original records
and decode or upcast into the current model only where the historical meaning
survives. Missing facts remain unknown; a conversion must not invent them.

Compatibility means preserving reconstructed state and subsequent command behavior,
not merely accepting a payload. Test with genuine historical records, then execute
current commands and reload the mixed history. Historical decoders do not require
parallel historical business implementations.

## Durable consumer versions

Names follow `<application>[--test]--<context>--<purpose>--v<major>`. The suffix
identifies subscription progress, not an application release or payload schema.
Preserve the exact deployed identity across compatible changes.

New durables normally start with `DeliverPolicy::All`. On streams permitting
overlapping consumers, a new generation can replay retained history. Integration
command IDs include durable identity, so that replay may repeat business actions
under new operation IDs. Choose a new generation only as an explicit migration
or replay, with a plan for pending work and retiring the old subscription.

Command work queues remove acknowledged commands and reject overlapping consumer
filters. An empty old queue does not release its filter; startup must not resolve
the conflict by silently deleting the durable. Stable names also do not make
incompatible queued payloads readable.

See [domain-event durable migration](domain-event-workers.md#existing-durable-migration)
and [shared-capacity changes](consumer-concurrency.md#existing-deployments-and-policy-changes).
