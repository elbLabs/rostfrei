# Ubiquitous Language

This is the canonical language for rostfrei's event-sourcing model. Framework
APIs, architecture decisions, documentation, and AI tooling use these terms with
the same meanings. ADR 0001 makes this language an architectural constraint.

## Domain model

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Aggregate** | A business consistency boundary whose state is reconstructed from its own history and whose command handlers make deterministic decisions. | Entity, record, model |
| **Aggregate identity** | The pair of aggregate type and aggregate ID that uniquely identifies one aggregate. | Key, record ID |
| **Aggregate state** | The transient state produced by replaying an aggregate stream to a selected version. | Stored row, source record |
| **Command** | A request for one aggregate to make a business decision. | Event, message, action |
| **Decision** | The deterministic outcome of handling a command: a rejection or an ordered set of new domain events. | Side effect, transaction |
| **Rejection** | An expected business outcome that declines a command and appends no domain events. | Error, exception, failure |
| **Domain event** | A private, meaningful fact that occurred in one aggregate and forms part of its authoritative history. | Message, notification, integration event |
| **Query** | A request to read state that never appends to an aggregate stream. | Read command, lookup command |

## Operational scope

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Application** | The top-level runtime and messaging namespace supplied once by an adopter. Its validated name is the first token in every rostfrei business, domain-event, and quarantine subject. | Owner, product prefix |
| **Bounded context** | A named domain language and ownership boundary inside one application. It scopes business addresses and authoritative domain-event storage. | Module, namespace, service |
| **Application messaging** | The derived command, integration-event, query, and quarantine conventions for one application. | Custom topology, message bus setup |

## History and execution

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Aggregate stream** | The permanent, ordered history of one aggregate identity. | Topic, queue, KV record, JetStream stream |
| **Stream version** | The one-based position of a domain event in an aggregate stream, with version zero representing no stream. | Revision, broker sequence |
| **Commit** | The atomic, non-empty ordered set of domain events produced by one accepted operation. | Message batch, transaction record |
| **Event transaction** | One atomic operation containing ordered commit or read-only participants from multiple aggregate streams in the same event store. | Distributed transaction, message batch |
| **Operation** | One identified request to execute a command against an aggregate stream, including the stable identity used for exact retry. | Delivery attempt, consumer attempt |
| **Command outcome** | The completed business result of command execution: accepted with a command receipt, or rejected with a modeled reason. | Execution status, handler result |
| **Command receipt** | The result detail for an accepted command: newly appended events, an exact replay, or no events. A no-events receipt is not durable in this release. | Publish receipt, broker acknowledgement |
| **Replay** | Reconstruction of aggregate state by applying its domain events in stream-version order. | Load row, restore snapshot |
| **EventStore** | The port that loads aggregate streams and atomically appends a commit or supported event transaction at expected versions. | Repository, KV store, message publisher |

## Public communication

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Integration event** | A bounded, independently versioned public contract normally derived from committed private domain events. | Domain event, raw event, notification |
| **Projection** | A read-oriented model derived from committed domain events without becoming aggregate truth. | Aggregate, source of truth |
| **Domain-event handler** | A post-commit application handler for one or more private domain events. It may perform side effects but never participates in aggregate decisions or changes the originating commit. | Projection handler, reaction, event projector |

## Relationships

- One **aggregate identity** identifies exactly one **aggregate stream**.
- One **application** contains one or more **bounded contexts**.
- Every business message address identifies one **application** and one **bounded context**.
- A **command** asks one **aggregate** to make a **decision**.
- A **rejection** produces no **commit**.
- An accepted **operation** that produces domain events creates exactly one **commit**.
- A **commit** contains one or more ordered **domain events**.
- An **event transaction** contains one or more **commits** and may guard other participating **aggregate streams** without appending to them.
- **Replay** reconstructs **aggregate state** at a selected **stream version**.
- A **query** reads state and never appends a **commit**.
- An **integration event** is a public contract separate from the private **domain events** from which it may be derived.
- A **projection** may consume committed **domain events**, but it does not replace the **aggregate stream** as authoritative history.

## Example dialogue

> **Developer:** "When `ApproveInvoice` arrives, do we publish an event directly?"
>
> **Domain expert:** "No. `ApproveInvoice` is a **command** asking the invoice
> **aggregate** to make a **decision**. If accepted, the decision records an
> `InvoiceApproved` **domain event** in one **commit**."
>
> **Developer:** "How do we know the invoice's state after that commit?"
>
> **Domain expert:** "We **replay** its **aggregate stream** through the new
> **stream version**. We do not load aggregate state from a KV record."
>
> **Developer:** "Can another bounded context consume `InvoiceApproved`?"
>
> **Domain expert:** "Not as a public contract. Application code derives a
> separately versioned **integration event** after the private domain event has
> committed."

## Flagged ambiguities

- **Event** is ambiguous. Use **domain event** for aggregate history and
  **integration event** for a public message contract.
- **Stream** is ambiguous. Use **aggregate stream** for one aggregate's logical
  history and **JetStream stream** for NATS infrastructure containing subjects.
- **Publish** is ambiguous. rostfrei **appends** domain events through the
  EventStore even though the NATS adapter uses broker publication internally;
  application messaging **publishes** commands and integration events.
- **Batch** is an implementation term. Use **commit** for the atomic business
  unit produced by an accepted operation.
- **Operation** and **command** are not synonyms. The command is the requested
  decision; the operation supplies the stable execution identity used for exact
  retry.
- **Rejection** is not an infrastructure failure. A rejection is an expected
  business outcome; storage, codec, and broker failures remain errors.
- **Command receipt** and **publish receipt** are not synonyms. A command receipt
  describes accepted aggregate execution; a publish receipt describes broker
  acknowledgement of an application message.
- **Aggregate state** is reconstructed state, not a NATS KV value or the
  authoritative persistence record.
- **Application name** is not a JetStream stream name. rostfrei derives stream
  names and subject filters from it.
