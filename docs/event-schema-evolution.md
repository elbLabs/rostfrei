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

Add a schema version for an incompatible change with a real compatibility
obligation, such as retained event history. Compatible changes and refactors do
not automatically need a new version. See [when to version a contract](messaging-upgrades.md#version-for-a-migration).

The example changes the stored payload incompatibly while retaining v1 records.
Its current events declare schema 2; the generated JSON codec writes that version
and reads only that exact schema:

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

Declare historical input types with their original metadata, outside the current
aggregate event enum. Put conversions in ordinary named Rust methods:

```rust
#[derive(Deserialize, rostfrei::DomainEvent)]
#[domain(id = "bicycle-registered", label = "Bicycle registered", schema_version = 1)]
struct BicycleRegisteredV1 {
    bicycle_id: String,
    needs_repair: bool,
}

struct InventoryCodec;

#[rostfrei::event_codec(aggregate = Inventory)]
impl InventoryCodec {
    #[upcast]
    fn registered_v1(old: BicycleRegisteredV1) -> BicycleRegistered {
        BicycleRegistered {
            bicycle_id: old.bicycle_id,
            condition: if old.needs_repair {
                Condition::MaintenanceRequired
            } else {
                Condition::Serviceable
            },
        }
    }
}
```

Install `rostfrei::install_macro_support!()` once at the crate root. The macro
adds `InventoryCodec::builder()`; build it once during application composition.
It selects historical schemas, deserializes, calls your method, and wraps the
current event. Other records use the generated strict JSON decoder. The methods
remain directly callable for focused tests and can use ordinary Rust helpers.

For fallible conversions, return `Result<CurrentEvent, Error>` from an `#[upcast]`
method. Malformed JSON is `MalformedPayload`; an application conversion error is
`ConversionFailed`; an unregistered version is `UnsupportedSchemaVersion`.
Errors retain the original record's context. Reconstruction fails rather than
skipping records or returning usable partial state.

The equivalent macro-free composition uses the same builder and validation:

```rust
let codec = rostfrei::EventCodecBuilder::<Inventory>::new()
    .upcast(InventoryCodec::registered_v1)
    .build()?;
```

Use `.try_upcast(method)` for a fallible method; its error must implement `Display`.
The built `HistoricalEventCodec<Inventory>` is an ordinary `EventCodec<Inventory>`.
This helper assumes one current schema per event ID, as generated `AggregateEvents`
enforces; use a hand-written codec when retaining same-ID legacy variants.
`build()` rejects duplicate mappings, overrides of the current schema, zero schema
versions, conflicting targets, and renamed event IDs. Conversion targets must
belong to the aggregate's event set. A hand-written `EventCodec` remains available
for formats outside this JSON composition API.

Macro methods are synchronous, non-generic, receiver-free functions with one owned
input and an explicit current-event or `Result<CurrentEvent, Error>` return type.
The macro cannot resolve `Result` aliases; use the explicit return syntax or
register that method manually with `.try_upcast(method)` without `#[upcast]`.
Use direct `#[cfg(...)]` for conditional methods; `cfg_attr` is not supported there.

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

Changing a conversion can change the meaning of a persisted read model. Rebuild
affected projections through their documented generation/cutover procedure;
deploying a new decoder does not reapply events already checkpointed.

## Register once

Register the codec in one registry and install that registry everywhere the
application decodes private events:

```rust
let mut codecs = AggregateCodecs::default();
codecs.register::<Inventory, _>(InventoryCodec::builder().build()?)?;
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
3. In the compatibility release, assert that new writes remain readable by v1 code
   and that unrepresentable values fail before committing any event or outcome.
4. Switch the writer, reload mixed history with both the new and compatibility
   readers, and assert that earlier records are unchanged.
5. Run the same scenario on real NATS with `scripts/test_nats.py`.

Successful deserialization alone is insufficient; assert state and subsequent
command behavior. Historical message-series fixtures may keep their v1 payloads:
`MessageSeriesEngine::with_codecs` validates them through the same registry and
rejects a malformed one before any write.

## Compatibility release

A **compatibility release** reads both old and new event schemas but keeps writing
the old format. It lets application instances be upgraded gradually:

| Release | Reads | Writes |
| --- | --- | --- |
| Original | v1 | v1 |
| Compatibility | v1 and v2 | v1 |
| New writer | v1 and v2 | v2 |

Deploy the compatibility release to every affected reader, including workers and
read models, before rolling out v2 writers. The write format can be fixed in each
release's code: `EventCodec::decode` accepts supported schemas independently of
the format emitted by `EventCodec::encode`.

The builder writes the aggregate's current JSON format by default. A compatibility
release supplies its own encoder, using the same declared readers:

```rust
let codec = InventoryCodec::builder()
    .with_encoder(encode_v1) // fn(&InventoryEvent, EventId) -> Result<NewEvent, EventCodecError>
    .build()?;
codecs.register::<Inventory, _>(codec)?;
```

The example's `encode_v1` writes the original typed payload and metadata. Selecting
readers never selects a writer implicitly, and no downcast is inferred.
Custom encoders must preserve the supplied event ID and event type.

Keep behavior requiring v2 information disabled while writing v1. Encoding must
reject values that the old format cannot represent without losing meaning.
After v2 events exist, roll back only to a release that can read them.

Upgrade technical-response consumers before enabling fallible upcasts: the new
`conversion_failed` codec failure kind travels in schema-2 responses, and older
readers reject that kind. See [technical failure compatibility](command-failures.md#responses-and-delivery).

Preserve stored records and subscription identities. Keep historical readers for
as long as retained history needs them, potentially indefinitely; an empty queue
does not make those readers obsolete.

## Limits

- The framework cannot judge whether a conversion or `Apply` change preserves
  business meaning. Maintained historical tests do.
- Each historical schema needs an explicit conversion to a current event; there
  are no automatic upcaster chains.
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
