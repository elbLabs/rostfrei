# Command execution and durable outcomes

**One operation, one durable decision.**

A retry, concurrent handler, lost reply, or different transport must not turn the
same operation into a new business attempt.

## What follows

- Use one processing contract for local, HTTP, NATS, and Tracer entry points.
  Commit events, read guards, and the winning decision atomically within one
  bounded-context store. Independent event and outcome writes are insufficient.
- Persist event-free acceptance and business rejection too. Rejection discards
  proposed events; every successful aggregate load still contributes a read guard.
  Stale guards require a bounded whole-handler retry after checking for a winner.
- Bind authority, context, and operation ID to canonical command evidence,
  provenance, security scope, and optional actor. Matching retries replay the
  winner; changed evidence conflicts. A new attempt needs a new operation ID.
- A decision never expires. Reclaiming result details or losing a response must
  never reopen execution. Broker deduplication and response caches are conveniences.
- Separate technical failure from business rejection. Reconcile an uncertain
  outcome using its original identity rather than issuing another operation.
- Authorize outcome reads before revealing a winner, and execution only after
  authoritative Missing. Trusted ingress supplies the caller; actor attribution
  does not authenticate it. Retain readers for durable rejection schemas.

## Tradeoff

Permanent identity and atomic coordination impose storage and adapter obligations.
They do not make external effects or transactions across contexts atomic; those
need explicit coordination and idempotency.

See [command runtime](../command-runtime.md), [failure recovery](../command-failures.md),
and [deployment cutover](../completed-outcome-cutover.md).
