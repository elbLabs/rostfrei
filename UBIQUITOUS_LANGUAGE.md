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
| **Traffic scope** | The routing and storage scope within one canonical application. Normal traffic is unsuffixed; isolated test traffic inserts the reserved `test` token and uses separate JetStream resources. | Environment name, second application |
| **Application messaging** | The derived command, integration-event, query, and quarantine conventions for one application. | Custom topology, message bus setup |

## History and execution

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Aggregate stream** | The permanent, ordered history of one aggregate identity. | Topic, queue, KV record, JetStream stream |
| **Stream version** | The one-based position of a domain event in an aggregate stream, with version zero representing no stream. | Revision, broker sequence |
| **Commit** | The atomic, non-empty ordered set of domain events produced by one accepted operation. | Message batch, transaction record |
| **Event transaction** | One atomic operation containing ordered commit or read-only participants from multiple aggregate streams in the same event store; its primary participant contributes a commit. | Distributed transaction, message batch |
| **Operation** | One identified request to execute a command against an aggregate stream, including the stable identity used for exact retry. | Delivery attempt, consumer attempt |
| **Replay** | Reconstruction of aggregate state by applying its domain events in stream-version order. | Load row, restore snapshot |
| **EventStore** | The port that loads aggregate streams and atomically appends a commit or supported event transaction at expected versions. | Repository, KV store, message publisher |

## Public communication

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Integration event** | A bounded, independently versioned public contract normally derived from committed private domain events. | Domain event, raw event, notification |
| **Integration-command mapping** | A consuming bounded context's pure mapping from one integration event to one typed command and target aggregate identity, dispatched under a stable durable identity. | Transport handler, event side effect |
| **Projection** | A read-oriented model derived from committed domain events without becoming aggregate truth. | Aggregate, source of truth |
| **Domain-event handler** | A post-commit application handler for one or more private domain events. It may perform side effects but never participates in aggregate decisions or changes the originating commit. | Projection handler, reaction, event projector |

## Tracer testing

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Tracer** | The interface for discovering domain contracts, executing commands, observing correlated behavior, and managing behavioral tests. | Admin panel, event viewer |
| **Domain catalog** | A versioned description of the commands, domain events, errors, and schemas exposed by a target application. | Registry, domain model |
| **Test definition** | A portable specification of setup, commands, and expectations independent of any particular runner or UI. | Test case, script |
| **Test repository** | The filesystem-backed, normally source-controlled collection of test definitions available to Tracer. | Test database, run history |
| **Test revision** | A source-controlled version of a test definition identified by its repository revision. | Database revision, draft event |
| **Test run** | One execution of a test revision against one target environment. | Test, operation |
| **Expectation** | A condition that correlated evidence must satisfy before a deadline. | Assertion, predicted event |
| **Test report** | The result returned by a test run, optionally exported as a CI artifact rather than retained by Tracer. | Operation result, trace history |
| **Correlation** | The identity shared by commands and events belonging to one business flow. | Test ID, operation ID |
| **Causation** | The direct parent relationship explaining which message caused another message. | Correlation, ordering |
| **Correlation trace** | The ordered commands, domain events, integration events, and outcomes observed for one correlation. | Event log, aggregate stream |
| **Test environment** | An isolated target system in which test runs may append state without affecting production. | Test mode, simulation |
| **Simulation** | A local command decision evaluated without appending its resulting domain events. | Test run, dry-run dispatch |
| **Production dispatch** | An explicitly authorized command execution against the production target system. | Test, simulation |

## Relationships

- One **aggregate identity** identifies exactly one **aggregate stream**.
- One **application** contains one or more **bounded contexts**.
- Every business message address identifies one **application** and one **bounded context**.
- Normal and test **traffic scopes** preserve the same **application** identity while using disjoint subjects and JetStream resources.
- A **command** asks one **aggregate** to make a **decision**.
- A **rejection** produces no **commit**.
- An accepted **operation** that produces domain events creates exactly one **commit**.
- A **commit** contains one or more ordered **domain events**.
- An **event transaction** contains one or more **commits** and may guard other participating **aggregate streams** without appending to them.
- **Replay** reconstructs **aggregate state** at a selected **stream version**.
- A **query** reads state and never appends a **commit**.
- An **integration event** is a public contract separate from the private **domain events** from which it may be derived.
- An **integration-command mapping** produces exactly one **command**; independent mappings use independent durable identities.
- A **projection** may consume committed **domain events**, but it does not replace the **aggregate stream** as authoritative history.
- A **test repository** contains zero or more **test definitions**, whose **test revisions** are supplied by source control.
- One **test run** executes exactly one **test revision** against one **test environment**.
- One **test run** produces exactly one terminal **test report**.
- A **test report** evaluates its **expectations** against a **correlation trace**.
- A **correlation** groups a business flow, while **causation** identifies direct parent-child relationships within that flow.
- The **domain catalog** describes possible contracts; a **correlation trace** records behavior that actually occurred.

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
>
> **Developer:** "Can I keep that complete behavior as a regression test?"
>
> **Domain expert:** "Yes. Save a **test definition** from the **correlation
> trace**, publish an immutable **test revision**, and execute a **test run** in
> the **test environment**. Its **test report** records which **expectations**
> the observed flow satisfied."

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
- **Aggregate state** is reconstructed state, not a NATS KV value or the
  authoritative persistence record.
- **Application name** is not a JetStream stream name. rostfrei derives stream
  names and subject filters from it.
- **Traffic scope** is not an application suffix. Use the canonical application
  name for normal traffic and the derived `test` token for isolated test traffic.
- **Test** is ambiguous. Use **test definition** for the specification, **test
  revision** for a saved version, **test run** for one execution, and **test
  environment** for the isolated target system.
- **Operation** and **test run** are not synonyms. An operation executes one
  command; a test run may coordinate multiple operations and expectations.
- **Correlation** and **causation** are not synonyms. Correlation groups the
  entire business flow; causation identifies the direct parent of one message.
- **Domain catalog** and **domain model** are not synonyms. The tested
  application owns its domain model and exposes the relevant contracts through
  a domain catalog.
