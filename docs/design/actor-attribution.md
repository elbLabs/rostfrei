# Original actor attribution

The [runtime stack and consolidation checklist](command-runtime-stack-and-consolidation.md)
shows how actor metadata travels through processing, persistence and result admission.

`ActorRef` records a claimed original actor as two opaque, case-sensitive UTF-8
strings: namespace (1–128 bytes) and id (1–256 bytes). Surrounding whitespace and
control characters are rejected. The shared type is exported by messaging-core,
core and the `rostfrei` facade.

At trusted ingress, attach attribution before encoding:

```rust,ignore
let request = CommandRequest::new(operation, domain_command)
    .with_actor(ActorRef::new("people", "stable-42")?);
let original = bus.encode(request)?;
let completed = service.process(&authenticated_context, &original).await.into_result()?;
assert_eq!(completed.outcome().marker().actor(), original.actor());
```

The current authenticated `PrincipalId` and authorization `SecurityScope` remain
separate. Actor is not a credential, role, permission grant or delegation proof.
An authorizer can inspect `ValidatedCommand::original().actor()`; a delivery
resolver can validate the original bytes against trusted ingress evidence.
`AllowAllCommands` and the explicit service-principal profile trust producers to
assert attribution; they do not establish that the named person acted. A different
authorized worker may replay the same original command. Missing actor means unknown;
it is never inferred from the worker, operator, security scope or payload.

The original actor survives evaluation, conflict retries, simulation metadata,
accepted/rejected/no-event/framework-rejected completions, replay, restart and detail
expiry. Private event handlers obtain it from `recorded().actor()`. The supported
private→public→command helpers preserve it automatically. Payload-only projections,
fixture plans and Tracer payload-only simulation do not invent attribution.

Adding, removing or changing actor under an existing operation is an identity
conflict after authorized outcome lookup, even with an unchanged caller-supplied
typed-command fingerprint. Operation keys, subjects, authorities, commit/event IDs
and integration source/consumer operation IDs retain their original derivations.
Raw transactions must match every batch's actor exactly; receipt reconstruction
checks every recorded event. Historical rejection import refuses attributed input.

## Formats and deployment

Actor-less serialization and hashes retain their original recipes. Actor-bearing
records use the following explicit versions, requiring a non-null actor:

| Boundary | Attributed version |
| --- | --- |
| Command/integration envelope metadata | 2 (domain schema unchanged) |
| Command fingerprint | 3 |
| Proposal, manifest, detail/header/reference | 2 |
| Permanent integrity hash / outcome revision hash | 2 / 3 |
| Native standalone / transactional event | 5 / 6 |
| Native receipt, raw/inline/split | 5 |
| Outer native Active/Expired state | 1, binding the complete versioned reference |

Old persistent formats reject any actor field, including null. Metadata version 2
without actor, actor with absent/legacy metadata version, null metadata versions and
unsupported versions are invalid. Actor-less and attributed operations coexist in
the same eligible authority and its existing two streams.

Use actor-capable producers, consumers and writers throughout an attributed route
at the existing explicit deployment cutover. Older binaries may discard unknown
integration metadata, so mixed old routes do not preserve attribution. This feature
does not migrate storage or enable old-writer rollback. Integration envelopes retain
their existing trusted-producer model; their stable IDs do not authenticate claims.
