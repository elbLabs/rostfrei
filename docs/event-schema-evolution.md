# Evolving private domain events

Stored domain events are facts that already happened. History is never rewritten,
so evolved code must keep reading every schema it ever wrote. Compatibility means
reconstructing the same state and behavior, not merely accepting the JSON.

The complete runnable example is
[`examples/event_evolution`](../examples/bike-rental/examples/event_evolution/main.rs)
(`cargo run -p bike-rental --example event_evolution`). Its domain and codec are in
[`inventory.rs`](../examples/bike-rental/examples/event_evolution/inventory.rs); the
maintained tests are in
[`tests/event_evolution.rs`](../examples/bike-rental/tests/event_evolution.rs).

```text
stored event type + schema + payload
  -> registered codec selects the historical reader / conversion
  -> typed current event
  -> deterministic Apply
  -> aggregate state
```

## Declare the current schema

Bump the declared schema of each changed event. The generated JSON codec writes
it and reads only that exact schema:

```rust
#[derive(Deserialize, Serialize, rostfrei::DomainEvent)]
#[domain(id = "bicycle-registered", label = "Bicycle registered", schema_version = 2)]
pub struct BicycleRegistered {
    pub bicycle_id: String,
    pub condition: Condition,
}
```

Keep the event ID. Bumping the schema alone makes earlier history unreadable;
reconstruction then fails with `UnsupportedSchemaVersion` instead of skipping it.

## Read history explicitly

Write one `EventCodec` per aggregate. Match the historical schemas it supports and
delegate everything else to the generated codec:

```rust
impl EventCodec<Inventory> for InventoryCodec {
    fn encode(&self, event: &InventoryEvent, id: EventId) -> Result<NewEvent, EventCodecError> {
        <JsonEventCodec as EventCodec<Inventory>>::encode(&JsonEventCodec, event, id)
    }

    fn decode(&self, event: &RecordedEvent) -> Result<InventoryEvent, EventCodecError> {
        match (event.event_type(), event.schema_version()) {
            ("bicycle-registered", 1) => {
                let v1: BicycleRegisteredV1 = historical(event)?;
                Ok(InventoryEvent::BicycleRegistered(BicycleRegistered {
                    bicycle_id: v1.bicycle_id,
                    condition: if v1.needs_repair {
                        Condition::MaintenanceRequired
                    } else {
                        Condition::Serviceable
                    },
                }))
            }
            ("bicycle-retired", 1) => {
                let v1: BicycleRetiredV1 = historical(event)?;
                Ok(InventoryEvent::BicycleRetired(BicycleRetired {
                    bicycle_id: v1.bicycle_id,
                    reason: RetirementReason::Unrecorded,
                }))
            }
            _ => <JsonEventCodec as EventCodec<Inventory>>::decode(&JsonEventCodec, event),
        }
    }
}
```

Convert to the current representation only where the meaning survives. v1
`needs_repair` maps exactly onto a condition. v1 retirements recorded no reason, so
the example keeps an explicit `RetirementReason::Unrecorded` instead of inventing
one. `Apply` and current commands then handle that distinction deliberately.

When no current value can express the historical fact, keep a separate legacy
event variant and give it its own `Apply` path. The generated event set requires
unique event IDs, so a v1 and a v2 variant of one event ID currently need a
hand-written event enum and codec. Typed support for that distinction is tracked
in [#111](https://github.com/elbLabs/rostfrei/issues/111).

The stored record stays v1 either way: `CommittedDomainEvent::recorded()` still
exposes its original schema, identity and bytes.

Decoding is pure. It never rewrites records, changes event or provenance
identities, runs command handlers, raises events, or publishes effects.

## Register once

Register the codec in one registry and install that registry everywhere the
application decodes private events:

```rust
let mut codecs = AggregateCodecs::default();
codecs.register::<Inventory, _>(InventoryCodec)?;
let codecs = Arc::new(codecs);

let mut bindings = ApplicationBindings::new().with_event_codecs(codecs.clone());
bindings.register_aggregate::<Inventory>()?;
let read_models = ReadModels::new(context, backend).with_event_codecs(codecs.clone());
```

`ApplicationBindings` passes it to execution (every participant of a multi-aggregate
command uses its own codec), fixtures, `simulate_handler` and post-commit
dispatch. An installed registry is complete: registering an aggregate without a
codec in it fails instead of falling back to generated JSON. A duplicate
registration fails; `replace` is the explicit override before sharing.

Applications without compatibility needs register nothing and keep the generated
JSON codec.

## Test with genuine history

Compatibility tests start from the bytes the previous release wrote, not from
current events re-encoded:

1. Append v1 records with their original type, schema and payload (`seed_v1_history`).
2. Reconstruct with the evolved codec and assert the expected state.
3. Execute a current command; assert that it writes the current schema.
4. Reload the mixed history, assert the state, and assert the v1 records are unchanged.
5. Run the same scenario on real NATS with `scripts/test_nats.py`.

Successful deserialization alone is insufficient; assert state and subsequent
command behavior. Historical message-series fixtures may keep their v1 payloads:
`MessageSeriesEngine::with_codecs` validates them through the same registry and
rejects a malformed one before any write.

## Roll out reader first

During a rolling deployment, old instances cannot read the new schema. When old
and new instances overlap, first release a reader that understands v2 while still
writing v1, then switch the writer. A single-step release is safe only when no old
instance remains.

## Limits

- The framework cannot judge whether a conversion or `Apply` change preserves
  business meaning. Maintained historical tests do.
- Each supported schema is an explicit match arm; there are no automatic upcaster
  chains.
- Post-commit handlers are routed by stored event type. Keep the event ID; a
  renamed historical event type does not reach the handler of the new name
  ([#111](https://github.com/elbLabs/rostfrei/issues/111)).
- Private schema, integration-event contract, crate release, stream position and
  durable subscription identity stay independent. A schema change never renames
  subjects or durables.
- Lower-level entry points stay independent of the shared registry by design:
  `CommandExecutor::new`, `CommandExecution::new` and an unregistered
  `AggregateCodecs` use generated JSON; `rehydrate_with_codec` and a standalone
  `DomainEventDispatcher::register_with_codec` use the codec they are given.
