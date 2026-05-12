# ADR 0019: Registered queries and standard HTTP endpoints

## Status

Accepted.

## Decision

`rostfrei` owns an application-facing `QueryBus` and `QueryProcessor` alongside
the existing command path. A query request type implements `QueryDefinition`,
which declares its bounded context, stable query name, schema version, and
response type. `DomainRegistry` stores query metadata deterministically and
applications register each query explicitly with `register_query::<Q>()`.

`QueryBus` prepares the existing transport-neutral query envelope, propagates
caller metadata and W3C trace context, and supports typed and dynamic requests.
`QueryProcessor` registers typed `QueryHandler` bindings and invokes them through
one erased JSON route without branching on query names. Query transport remains
request/reply rather than durable command delivery; the existing NATS query
requester and server remain the broker adapters. Registered application queries
may invoke compiled-domain query functions, projections, or other read models,
but `#[domain_query]` remains the declaration of synchronous domain reads and
does not itself select an application transport.

`rostfrei-http` is a separate optional application-edge adapter. An application
explicitly mounts a router with a shared `DomainRegistry`, `CommandBus`, and
`QueryBus`. The router exposes only metadata registered in that registry:

```text
POST /contexts/{context}/queries/{query}/schemas/{schema_version}
POST /contexts/{context}/aggregates/{aggregate}/{aggregate_id}/commands/{command}/schemas/{schema_version}
```

POST query bodies contain the raw query JSON payload and require
`Content-Type: application/json`. The decoded JSON value is passed unchanged to
the dynamic query request and query bus, so structured and nested inputs retain
their exact JSON representation. Query success returns the raw JSON result with
`200 OK`.

POST command bodies contain the raw command JSON payload. `Idempotency-Key` is
mandatory and becomes the command operation ID. Because `CommandBus` waits for
the terminal durable command response, accepted commands return `200 OK` rather
than Tracer's asynchronous `202 Accepted` operation representation. Business
query errors and command rejections map their standard classifications to HTTP
status codes.

The HTTP adapter extracts query trace context and applies conservative
`Cache-Control: private, no-store` response policy. It does not own
authentication or authorization; applications protect the mounted router with
their own middleware. Applications that permit handlers to return an
`Unauthorized` classification configure the corresponding `WWW-Authenticate`
challenge; without one, that classification becomes `403 Forbidden` so the
adapter never emits a protocol-invalid `401`. Command trace-context propagation
remains deferred until the command envelope has a protocol-neutral trace-context
field.

## Consequences

Queries now have the same metadata, typed application API, dynamic boundary,
processor registration, and in-memory/NATS portability as commands without
acquiring command durability or aggregate-mutation semantics. HTTP exposure is
standardized and opt-in at the composition root, while business handlers remain
independent from Axum and HTTP status codes.

Metadata registration and executable binding remain separate. A registered
route whose deployment omitted its processor binding receives the normal
framework unknown-query or unknown-command outcome rather than silently
selecting a handler or infrastructure environment.

Queries remain safe and idempotent by application contract even though POST is
used to carry their JSON payload; query handlers must not mutate application
state. The generic POST representation favors a stable, lossless dynamic
contract over framework-specific typed extractors. Applications needing
resource-shaped URLs, custom caching, or alternate status semantics should
place a purpose-built HTTP adapter in front of the same buses.
