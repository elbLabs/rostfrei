# Ubiquitous Language

This is the canonical language for rostfrei's event-sourcing model. Framework
APIs, architecture decisions, documentation, and AI tooling use these terms with
the same meanings. ADR 0001 makes this language an architectural constraint.

## Domain model

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Aggregate** | A business consistency boundary whose state is reconstructed from its own history and whose domain behavior makes deterministic decisions. | Entity, record, model |
| **Aggregate identity** | A bounded-context-qualified aggregate type and aggregate ID that identify one aggregate within an application. | Key, record ID |
| **Aggregate state** | The transient state produced by replaying an aggregate stream to a selected version. | Stored row, source record |
| **Command** | A self-contained request for a bounded-context use case to make a business decision. | Event, message, action |
| **Decision** | The deterministic outcome of handling a command: rejection or acceptance with zero or more new domain events. | Side effect, transaction |
| **Domain policy** | A reusable, pure interpretation of domain facts that returns a business outcome without recording events or changing state. | Decision, rule service |
| **Rejection** | An expected business outcome that declines a command and appends no domain events. | Error, exception, failure |
| **Domain event** | A private, meaningful fact that occurred in one aggregate and forms part of its authoritative history. | Message, notification, integration event |
| **Query** | A read-only request that does not mutate application state. | Read command, lookup command |

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
| **Commit** | The non-empty ordered set of domain events appended atomically to one aggregate stream by an accepted operation. | Message batch, transaction record |
| **Event transaction** | One atomic command acceptance containing a receipt and zero or more writing or read-only aggregate participants in the same event store. | Distributed transaction, message batch |
| **Operation** | One identified request to execute a bounded-context command, including the stable identity used for exact retry. | Delivery attempt, consumer attempt |
| **Replay** | Reconstruction of aggregate state by applying its domain events in stream-version order. | Load row, restore snapshot |
| **EventStore** | The port that loads aggregate streams and atomically appends a commit or supported event transaction at expected versions. | Repository, KV store, message publisher |

## Public communication

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Integration event** | A bounded, independently versioned public contract normally derived from committed private domain events. | Domain event, raw event, notification |
| **Integration-command mapping** | A consuming bounded context's pure mapping from one integration event to one self-contained typed command, dispatched under a stable durable identity. | Transport handler, event side effect |
| **Projection** | A read-oriented model derived from committed domain events without becoming aggregate truth. | Aggregate, source of truth |
| **Read model** | An application-owned, query-oriented materialization whose authoritative inputs may be committed domain events, integration events, or external facts. | Aggregate state, source of truth |
| **Storage revision** | An opaque compare-and-set token for one persisted read-model value. | Stream version, source checkpoint |
| **Source checkpoint** | Application-owned progress for a read model's input source, persisted with the corresponding materialized value. | Storage revision, delivery attempt |
| **Domain-event handler** | A post-commit application handler for one or more private domain events. It may perform side effects but never participates in aggregate decisions or changes the originating commit. | Projection handler, reaction, event projector |

## Tracer testing

| Term | Definition | Aliases to avoid |
| --- | --- | --- |
| **Tracer** | The interface for discovering domain contracts, executing commands, observing correlated behavior, and managing behavioral tests. | Admin panel, event viewer |
| **Domain catalog** | A versioned description of the commands, domain events, errors, and schemas exposed by a target application. | Registry, domain model |
| **Test definition** | A portable specification of setup, commands, and expectations independent of any particular runner or UI. | Test case, script |
| **Test repository** | The filesystem-backed, normally source-controlled collection of test definitions available to Tracer. | Test database, run history |
| **Test revision** | An identified version of a test definition; the filesystem repository uses a content fingerprint. | Database revision, draft event |
| **Test run** | One execution of a test revision against one target environment. | Test, operation |
| **Expectation** | A condition that correlated evidence must satisfy before a deadline. | Assertion, predicted event |
| **Test report** | The result returned by a test run, optionally exported as a CI artifact rather than retained by Tracer. | Operation result, trace history |
| **Correlation** | The identity shared by commands and events belonging to one business flow. | Test ID, operation ID |
| **Causation** | The direct parent relationship explaining which message caused another message. | Correlation, ordering |
| **Correlation trace** | The commands, domain events, integration events, and outcomes observed for one correlation. Observation order alone does not establish causality. | Event log, aggregate stream |
| **Test environment** | An isolated target system in which test runs may append state without affecting production. | Test mode, simulation |
| **Handler simulation** | Core test primitive that evaluates a handler without appending authoritative domain events or an acceptance receipt. Not a Tracer action or a sandbox for arbitrary injected dependencies. | Test run, dry-run dispatch |
| **Production dispatch** | An explicitly authorized command execution against the production target system. | Test, simulation |

## Relationships

- One **aggregate identity** identifies exactly one **aggregate stream**.
- One **application** contains one or more **bounded contexts**.
- Every business message address identifies one **application** and one **bounded context**.
- Normal and test **traffic scopes** preserve the same **application** identity while using disjoint subjects and JetStream resources.
- A **command** asks one **bounded context** to execute one use case and may coordinate one or more **aggregates**.
- A **decision** may consult one or more **domain policies**, but a policy does not itself accept or reject a command or record domain events.
- A **rejection** produces no **commit**.
- An accepted **operation** persists an **event transaction**, including when it produces no domain events.
- Each writing participant in an **event transaction** contributes one **commit**.
- A **commit** contains one or more ordered **domain events**.
- An **event transaction** contains zero or more **commits** and may guard participating **aggregate streams** without appending to them. A receipt-only transaction loads no aggregates.
- **Replay** reconstructs **aggregate state** at a selected **stream version**.
- A **query** reads state and never appends a **commit**.
- An **integration event** is a public contract separate from the private **domain events** from which it may be derived.
- An **integration-command mapping** produces exactly one **command**; independent mappings use independent durable identities.
- A **projection** may consume committed **domain events**, but it does not replace the **aggregate stream** as authoritative history.
- A **test repository** contains zero or more **test definitions** and identifies their **test revisions**.
- One **test run** executes exactly one **test revision** against one **test environment**.
- One **test run** produces exactly one terminal **test report**.
- A **test report** evaluates its **expectations** against a **correlation trace**.
- A **correlation** groups a business flow, while **causation** identifies direct parent-child relationships within that flow.
- The **domain catalog** describes possible contracts; a **correlation trace** records behavior that actually occurred.

## Terms to distinguish

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
- **Decision** and **domain policy** are not synonyms. A decision is the
  command-scoped result; a policy is a reusable, pure interpretation that a
  decision, action, or query may consult.
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
