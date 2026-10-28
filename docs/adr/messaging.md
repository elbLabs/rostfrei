# Messaging and traffic scopes

**Share explicit contracts, not private history. Version for real migrations.**

Contexts need to evolve independently. Neither a private event shape nor an
implementation iteration should become a permanent obligation for every consumer.

## What follows

- Separate private domain events from independently versioned integration events.
  Publish public facts after commit through durable handlers; make external
  effects idempotent because delivery is at least once.
- Let typed buses own canonical envelopes, addresses, correlation, causation, and
  identities. Adapters own transport delivery, responses, and quarantine.
  Derive downstream operation identity from the source and subscription identity.
- Keep one current business implementation. Add compatibility only for concrete
  deployed callers, deliverable messages, or retained history. Adapt older
  payloads at the boundary where their meaning permits; do not copy workflows
  for every schema. Preserve historical domain-event readers as long as needed.
- Every rolling schema migration needs reader-first rollout and an observable
  retirement condition. Versions are not feature or release counters. Subscription
  generation is a separate decision: changing it can repeat business actions.
- Derive topology from application, scope, and context. Keep Normal and Test
  resources disjoint, restrict reset to Test, and grant credentials accordingly.
  A correlation ID is not an isolation boundary.
- Make provisioning and consumer migrations explicit. Startup verifies topology
  rather than silently deleting, recreating, or changing resources.

## Tradeoff

Explicit mapping and migration work buy independent contracts. Post-commit
publication is retried but is not atomic with event storage; bounded broker
deduplication does not remove the need for downstream idempotency.

See [subjects](../messaging/subjects.mdx),
[schema and subscription upgrades](../messaging-upgrades.md),
[domain-event workers](../domain-event-workers.md), and
[shared worker capacity](../consumer-concurrency.md).
