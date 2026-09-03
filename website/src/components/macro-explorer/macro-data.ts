export type MacroSlide = {
  name: string
  family: string
  headline: string
  description: string
  points: string[]
  file: string
  authored: string
  generated: string
}

export const MACROS: MacroSlide[] = [
  {
    name: "BoundedContext",
    family: "derive",
    headline: "Name one domain language.",
    description:
      "A bounded context is a small marker with a stable ID and a human label.",
    points: [
      "Non-generic unit struct",
      "Stable context ID",
      "No runtime behavior",
    ],
    file: "src/domain/bike_rental/context.rs",
    authored: `#[derive(BoundedContext)]
#[domain(id = "bike-rental", label = "Bike rental")]
pub struct BikeRental;`,
    generated: `impl BoundedContextType for BikeRental {
    const DESCRIPTOR: BoundedContextDescriptor =
        BoundedContextDescriptor {
            id: BoundedContextId("bike-rental"),
            label: "Bike rental",
        };
}`,
  },
  {
    name: "Aggregate",
    family: "derive",
    headline: "Keep aggregate metadata small.",
    description:
      "The derive names the aggregate. An ordinary definition trait keeps context, root, and event relationships visible.",
    points: [
      "ID and label only",
      "Relationships stay authored",
      "Runtime bridge generated",
    ],
    file: "src/domain/bike_rental/rental_fleet/aggregate.rs",
    authored: `#[derive(Aggregate)]
#[domain(id = "rental-fleet", label = "Rental fleet")]
pub struct RentalFleetAggregate;

impl AggregateDefinition for RentalFleetAggregate {
    type Context = BikeRental;
    type Root = RentalFleet;
    type Event = RentalFleetEvent;
}`,
    generated: `impl AggregateType for RentalFleetAggregate {
    const DESCRIPTOR: AggregateDescriptor =
        /* context-scoped ID and root metadata */;
}

impl rostfrei_core::Aggregate for RentalFleetAggregate {
    type State = RentalFleet;
    type Event = RentalFleetEvent;
    // initialize and apply delegate to authored traits
}`,
  },
  {
    name: "AggregateEvents",
    family: "derive",
    headline: "One closed set owns the stream.",
    description:
      "The event enum is the single authority for membership, serialization, replay, and Apply dispatch.",
    points: [
      "One tuple variant per event",
      "JSON codec and replay",
      "Unregistered events fail to compile",
    ],
    file: "src/domain/bike_rental/rental_fleet/event_set.rs",
    authored: `#[derive(AggregateEvents)]
pub enum RentalFleetEvent {
    BicycleAdded(BicycleAdded),
    BicycleRented(BicycleRented),
    BicycleReturned(BicycleReturned),
}`,
    generated: `impl From<BicycleRented> for RentalFleetEvent { /* … */ }
impl EventVariant<BicycleRented> for RentalFleetEvent { /* … */ }

impl<A> AggregateEventSet<A> for RentalFleetEvent
where A: AggregateDefinition<Event = Self>
{
    const DOMAIN_EVENTS: &[DomainEventDescriptor] = /* … */;
}

// Also generates JSON codec, replay checks,
// and Apply dispatch for every listed event.`,
  },
  {
    name: "Entity",
    family: "derive",
    headline: "State plus an explicit identity.",
    description:
      "Entity metadata is derived locally. Owner, identity type, and identity access remain compiler-checked Rust.",
    points: [
      "Named-field struct",
      "Identity accessor required",
      "Custom fields stay opaque",
    ],
    file: "src/domain/bike_rental/rental_fleet/bicycle/entity.rs",
    authored: `#[derive(Entity)]
#[domain(id = "bicycle", label = "Bicycle")]
pub struct Bicycle {
    bicycle_id: BicycleId,
    status: BicycleStatus,
}

impl EntityDefinition for Bicycle {
    type Owner = RentalFleetAggregate;
    type Identity = BicycleId;

    fn identity(&self) -> &Self::Identity {
        &self.bicycle_id
    }
}`,
    generated: `impl EntityType for Bicycle {
    const LOCAL_ID: &str = "bicycle";
    const DESCRIPTOR: EntityDescriptor = EntityDescriptor {
        id: /* aggregate-scoped EntityId */,
        label: "Bicycle",
        identity: /* entity-scoped DomainIdentityId */,
        fields: /* scalar or opaque metadata */,
    };
}`,
  },
  {
    name: "DomainIdentity",
    family: "derive",
    headline: "Any Rust type can be an identity.",
    description:
      "The derive is deliberately only a semantic marker. The EntityDefinition accessor gives it meaning.",
    points: [
      "Struct or enum",
      "No scalar declaration",
      "UUIDs and custom types work",
    ],
    file: "src/domain/bike_rental/rental_fleet/bicycle/identity.rs",
    authored: `#[derive(DomainIdentity, Clone, Debug, Eq, PartialEq)]
pub struct BicycleId(uuid::Uuid);`,
    generated: `impl DomainIdentity for BicycleId {}

// No owner, string conversion, scalar metadata,
// or generated storage behavior.`,
  },
  {
    name: "ValueObject",
    family: "derive",
    headline: "Tag only genuinely semantic values.",
    description:
      "A Value Object keeps a global semantic ID and label without claiming a field shape or transport schema.",
    points: [
      "Struct or enum",
      "ID and label only",
      "Operation DTOs stay ordinary Rust",
    ],
    file: "src/domain/bike_rental/rental_fleet/bicycle/status.rs",
    authored: `#[derive(ValueObject, Clone, Copy, Debug)]
#[domain(id = "bicycle-status", label = "Bicycle status")]
pub enum BicycleStatus {
    Available,
    Rented,
}`,
    generated: `impl ValueObject for BicycleStatus {
    const DESCRIPTOR: ValueObjectDescriptor =
        ValueObjectDescriptor {
            id: ValueObjectId("bicycle-status"),
            label: "Bicycle status",
        };
}`,
  },
  {
    name: "DomainService",
    family: "derive",
    headline: "Put cross-entity behavior in context.",
    description:
      "The derive names the service; DomainServiceDefinition supplies the only required relationship.",
    points: [
      "Non-generic unit struct",
      "Context relationship is explicit",
      "No implicit actions",
    ],
    file: "src/domain/bike_rental/fleet_planning/service.rs",
    authored: `#[derive(DomainService)]
#[domain(id = "fleet-planning", label = "Fleet planning")]
pub struct FleetPlanning;

impl DomainServiceDefinition for FleetPlanning {
    type Context = BikeRental;
}`,
    generated: `impl DomainServiceType for FleetPlanning {
    const DESCRIPTOR: DomainServiceDescriptor =
        DomainServiceDescriptor {
            id: /* context-scoped service ID */,
            label: "Fleet planning",
        };
}`,
  },
  {
    name: "Command",
    family: "derive",
    headline: "One JSON path for commands.",
    description:
      "Commands carry boundary payload metadata and an exact JSON codec. The handler supplies aggregate and rejection relationships.",
    points: [
      "Schema version 1 by default",
      "Exact JSON shape",
      "Owner-independent payload",
    ],
    file: "src/domain/bike_rental/rental_fleet/rent_bicycle/command.rs",
    authored: `#[derive(Command, Clone, Debug)]
#[domain(id = "rent-bicycle", label = "Rent bicycle")]
pub struct RentBicycle {
    pub bicycle_id: BicycleId,
}`,
    generated: `impl Command for RentBicycle {
    const LOCAL_ID: &str = "rent-bicycle";
    const LABEL: &str = "Rent bicycle";
    const SCHEMA_VERSION: u32 = 1;
    const FIELDS: &[FieldDescriptor] = /* … */;
}

impl JsonCommandPayload for RentBicycle {
    fn encode_json(&self) -> Result<Value, String> { /* … */ }
    fn decode_json(value: &Value) -> Result<Self, String> { /* … */ }
}`,
  },
  {
    name: "DomainEvent",
    family: "derive",
    headline: "Events describe facts, not owners.",
    description:
      "An event owns intrinsic wire metadata. AggregateEvents supplies its aggregate membership and runtime codec.",
    points: [
      "Schema version 1 by default",
      "Serializable Rust payload",
      "Membership defined once",
    ],
    file: "src/domain/bike_rental/rental_fleet/rent_bicycle/event.rs",
    authored: `#[derive(DomainEvent, Serialize, Deserialize)]
#[domain(id = "bicycle-rented", label = "Bicycle rented")]
pub struct BicycleRented {
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
}`,
    generated: `impl DomainEvent for BicycleRented {
    const LOCAL_ID: &str = "bicycle-rented";
    const LABEL: &str = "Bicycle rented";
    const FIELDS: &[FieldDescriptor] = /* … */;
    // SCHEMA_VERSION uses the trait default: 1
}`,
  },
  {
    name: "DomainError",
    family: "derive",
    headline: "Make business rejection stable.",
    description:
      "A Domain Error owns its public code and message and always gets the conventional JSON rejection encoding.",
    points: [
      "Global semantic ID",
      "Stable code and message",
      "JSON encoding by default",
    ],
    file: "src/domain/bike_rental/rental_fleet/rent_bicycle/rejection.rs",
    authored: `#[derive(DomainError, Clone, Debug)]
#[domain(
    id = "bicycle-unavailable",
    label = "Bicycle unavailable",
    code = "BICYCLE_UNAVAILABLE",
    message = "The bicycle cannot currently be rented."
)]
pub struct BicycleUnavailable {
    pub bicycle_id: BicycleId,
}`,
    generated: `impl DomainError for BicycleUnavailable {
    const LOCAL_ID: &str = "bicycle-unavailable";
    const LABEL: &str = "Bicycle unavailable";
    const CODE: &str = "BICYCLE_UNAVAILABLE";
    const MESSAGE: &str = "The bicycle cannot currently be rented.";
    const FIELDS: &[FieldDescriptor] = /* … */;
}

impl JsonErrorPayload for BicycleUnavailable { /* … */ }`,
  },
  {
    name: "DecisionOutcome",
    family: "derive",
    headline: "Keep outcomes closed and exhaustive.",
    description:
      "The enum remains ordinary Rust while each business outcome receives a stable local ID and label.",
    points: [
      "Non-empty enum",
      "Ordered outcome metadata",
      "Arbitrary Rust payloads",
    ],
    file: "src/domain/bike_rental/rental_fleet/assess_rental_eligibility/outcome.rs",
    authored: `#[derive(DecisionOutcome, Clone, Copy, Debug)]
pub enum RentalEligibilityOutcome {
    #[outcome(id = "eligible", label = "Eligible")]
    Eligible,
    #[outcome(id = "already-rented", label = "Already rented")]
    AlreadyRented,
}`,
    generated: `impl DecisionOutcomeType for RentalEligibilityOutcome {
    const OUTCOMES: &[DecisionOutcomeDescriptor] = &[
        DecisionOutcomeDescriptor {
            local_id: "eligible",
            label: "Eligible",
        },
        /* … */
    ];
}`,
  },
  {
    name: "EntityLifecycle",
    family: "derive",
    headline: "Name lifecycle states without inventing behavior.",
    description:
      "Lifecycle metadata is an ordered vocabulary. Initial state, transitions, and ownership remain intentionally unspecified.",
    points: [
      "Fieldless enum",
      "Ordered state metadata",
      "No transition engine yet",
    ],
    file: "src/domain/bike_rental/rental_fleet/bicycle/rental_status/lifecycle.rs",
    authored: `#[derive(EntityLifecycle)]
#[domain(id = "rental-status", label = "Rental status")]
pub enum BicycleRentalLifecycle {
    #[state(id = "available", label = "Available")]
    Available,
    #[state(id = "rented", label = "Rented")]
    Rented,
}`,
    generated: `impl EntityLifecycleType for BicycleRentalLifecycle {
    const DESCRIPTOR: EntityLifecycleDescriptor =
        EntityLifecycleDescriptor {
            id: EntityLifecycleId("rental-status"),
            label: "Rental status",
            states: &[/* available, rented */],
        };
}`,
  },
  {
    name: "domain_action",
    family: "behavior",
    headline: "Keep actions as ordinary traits.",
    description:
      "The macro preserves the trait and adds only owner-independent metadata. The checker links its implementation through nesting.",
    points: [
      "No generated second trait",
      "No prescribed signature",
      "Direct method calls remain",
    ],
    file: "src/domain/bike_rental/rental_fleet/rent_bicycle/action.rs",
    authored: `#[domain_action(id = "rent-bicycle", label = "Rent bicycle")]
pub trait RentBicycleAction {
    fn rent_bicycle(
        &mut self,
        bicycle_id: BicycleId,
    ) -> Result<(), BicycleUnavailable>;
}`,
    generated: `pub trait RentBicycleAction {
    fn rent_bicycle(
        &mut self,
        bicycle_id: BicycleId,
    ) -> Result<(), BicycleUnavailable>;

    const LOCAL_ID: &str = "rent-bicycle";
    const LABEL: &str = "Rent bicycle";
    const DESCRIPTOR: ActionDescriptor = /* … */;
}`,
  },
  {
    name: "domain_query",
    family: "behavior",
    headline: "Read state through a normal trait.",
    description:
      "A query stays a normal method on the aggregate root. The checker proves that execute.rs implements the correct trait for that root.",
    points: [
      "Root is the receiver",
      "Inputs and outputs are ordinary Rust",
      "No model group",
    ],
    file: "src/domain/bike_rental/rental_fleet/bicycle_availability/query.rs",
    authored: `#[domain_query(
    id = "bicycle-availability",
    label = "Bicycle availability"
)]
pub trait BicycleAvailabilityQuery {
    fn bicycle_availability(
        &self,
        bicycle_id: &BicycleId,
    ) -> Option<BicycleAvailability>;
}`,
    generated: `pub trait BicycleAvailabilityQuery {
    fn bicycle_availability(
        &self,
        bicycle_id: &BicycleId,
    ) -> Option<BicycleAvailability>;

    const LOCAL_ID: &str = "bicycle-availability";
    const LABEL: &str = "Bicycle availability";
    const DESCRIPTOR: QueryDescriptor = /* … */;
}`,
  },
  {
    name: "domain_decision",
    family: "behavior",
    headline: "Make policy explicit and reusable.",
    description:
      "A decision is a singular trait with arbitrary Rust inputs and a closed outcome vocabulary when the domain needs one.",
    points: [
      "Aggregate or entity implementation",
      "No owner metadata",
      "No projection group",
    ],
    file: "src/domain/bike_rental/rental_fleet/assess_rental_eligibility/decision.rs",
    authored: `#[domain_decision(
    id = "assess-rental-eligibility",
    label = "Assess rental eligibility"
)]
pub trait RentalEligibilityDecision {
    fn assess(
        status: BicycleStatus,
        condition: BicycleCondition,
    ) -> RentalEligibilityOutcome;
}`,
    generated: `pub trait RentalEligibilityDecision {
    fn assess(
        status: BicycleStatus,
        condition: BicycleCondition,
    ) -> RentalEligibilityOutcome;

    const LOCAL_ID: &str = "assess-rental-eligibility";
    const LABEL: &str = "Assess rental eligibility";
    const DESCRIPTOR: DecisionDescriptor = /* … */;
}`,
  },
  {
    name: "domain_invariant",
    family: "behavior",
    headline: "Separate a rule from its evaluation.",
    description:
      "The contract names one invariant. evaluate.rs supplies its ordinary Rust implementation for the enclosing aggregate or entity.",
    points: [
      "Global semantic ID",
      "Arbitrary validation signature",
      "InvariantViolation stays explicit",
    ],
    file: "src/domain/bike_rental/rental_fleet/fleet_consistency/contract.rs",
    authored: `#[domain_invariant(
    id = "unique-bicycle-identities",
    label = "Bicycle identities are unique"
)]
pub trait FleetConsistency {
    fn validate(
        candidate: &RentalFleet,
    ) -> Option<InvariantViolation>;
}`,
    generated: `pub trait FleetConsistency {
    fn validate(
        candidate: &RentalFleet,
    ) -> Option<InvariantViolation>;

    const LOCAL_ID: &str = "unique-bicycle-identities";
    const LABEL: &str = "Bicycle identities are unique";
    const DESCRIPTOR: InvariantDescriptor = /* … */;
}`,
  },
  {
    name: "domain_*_test",
    family: "test",
    headline: "Attach evidence to its subject.",
    description:
      "Action, query-independent decision, invariant, and lifecycle tests remain normal Rust tests with discoverable subject metadata.",
    points: [
      "Authored body stays unchanged",
      "Typed descriptor expression",
      "Ignored discovery companion",
    ],
    file: "src/domain/tests/bike_rental/rental_fleet/fleet_consistency.rs",
    authored: `#[domain_invariant_test(
    <RentalFleetAggregate as FleetConsistency>::DESCRIPTOR
)]
fn duplicate_bicycles_are_rejected() {
    // arrange, evaluate, assert
}`,
    generated: `#[test]
fn duplicate_bicycles_are_rejected() {
    // authored body unchanged
}

const __DOMAIN_TEST_SUBJECT: DomainTestSubject =
    DomainTestSubject::Invariant(
        (<RentalFleetAggregate as FleetConsistency>
            ::DESCRIPTOR).id,
    );

// An ignored companion emits discovery metadata.`,
  },
  {
    name: "domain_model!",
    family: "model",
    headline: "Keep model composition visible.",
    description:
      "The explicit inventory builds one deterministic catalog. Events and identities are discovered through their aggregate and entity relationships.",
    points: [
      "One visible composition root",
      "Returns validated JSON",
      "No hidden behavior inventory",
    ],
    file: "src/domain/model.rs",
    authored: `domain_model! {
    contexts: [BikeRental],
    aggregates: [RentalFleetAggregate],
    entities: [RentalFleet, Bicycle],
    value_objects: [BicycleStatus, BicycleCondition],
    services: [],
    errors: [BicycleUnavailable],
}`,
    generated: `try_build(|builder| {
    builder.add_bounded_context(BikeRental::DESCRIPTOR);
    builder.add_aggregate_type::<RentalFleetAggregate>()?;
    builder.add_entity_type::<RentalFleet>()?;
    builder.add_entity_type::<Bicycle>()?;
    builder.add_value_object_type::<BicycleStatus>()?;
    builder.add_domain_error(BicycleUnavailable::DESCRIPTOR)?;
    Ok(())
})`,
  },
]
