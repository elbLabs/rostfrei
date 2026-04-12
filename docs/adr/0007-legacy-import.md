# ADR 0007: Legacy state import and provenance

## Status

Accepted.

## Decision

Legacy state enters an empty stream through an honest domain event such as
`AccountStateImported`, `ExistingInvoiceLinked`, or another domain-accepted
starting fact. The event payload records enough provenance to distinguish
native, imported, synthetic, and externally observed facts, including source
system, source record, observation time, and import batch where applicable.

Rostfrei does not fabricate historical business events from a current-state
row and does not hide imported state in a snapshot. Import is a normal
`NoStream` commit and follows the same concurrency and idempotency rules as any
other command.

## Consequences

Aggregate history remains truthful and replayable. Provenance is meaningful
domain data selected by each bounded context, rather than an untyped framework
metadata bag that aggregates cannot interpret.
