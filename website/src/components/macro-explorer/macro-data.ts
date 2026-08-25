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
    name: "install_macro_support!",
    family: "setup",
    headline: "Install macro support once.",
    description:
      "One crate-root call gives every Rostfrei macro a deterministic path to its domain, runtime, and registry support.",
    points: [
      "One call per declaring crate",
      "Safe with renamed dependencies",
      "No per-type path configuration",
    ],
    file: "src/lib.rs",
    authored: `rostfrei::install_macro_support!();

pub mod domain;`,
    generated: `#[doc(hidden)]
pub mod __rostfrei_macro_support {
    // Hygienic re-exports supplied by rostfrei.
    // Domain macros target this crate-local bridge.
}`,
  },
  {
    name: "BoundedContext",
    family: "derive",
    headline: "Declare a bounded context.",
    description:
      "A unit struct gives commands, aggregates, and services a shared context ID and display label.",
    points: [
      "Non-generic unit struct",
      "Stable context ID",
      "No runtime behavior",
    ],
    file: "src/domain/bike_rental/context.rs",
    authored: `#[derive(BoundedContext)]
#[domain(id = "bike-rental", label = "Bike Rental")]
pub struct BikeRental;`,
    generated: `impl BoundedContextType for BikeRental {
    const DESCRIPTOR: BoundedContextDescriptor =
        BoundedContextDescriptor {
            id: BoundedContextId("bike-rental"),
            label: "Bike Rental",
        };
}`,
  },
  {
    name: "Aggregate",
    family: "derive",
    headline: "Connect context, state, and events.",
    description:
      "The derive names the aggregate. An ordinary definition trait keeps context, root, and event relationships visible.",
    points: [
      "ID and label only",
      "Explicit AggregateDefinition",
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
    headline: "Declare the aggregate's event set.",
    description:
      "Each tuple variant selects an event type that the aggregate can record and replay.",
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
    BicycleRetired(BicycleRetired),
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
    condition: BicycleCondition,
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
    headline: "Mark a domain identity type.",
    description:
      "Mark a struct or enum, then expose its value through EntityDefinition::identity().",
    points: [
      "Struct or enum",
      "No scalar declaration",
      "UUIDs and custom types work",
    ],
    file: "src/domain/bike_rental/rental_fleet/bicycle/identity.rs",
    authored: `#[derive(DomainIdentity, Clone, Debug, Eq, PartialEq)]
pub struct BicycleId(String);`,
    generated: `impl DomainIdentity for BicycleId {}

// No owner, string conversion, scalar metadata,
// or generated storage behavior.`,
  },
  {
    name: "ValueObject",
    family: "derive",
    headline: "Name a domain value.",
    description:
      "Adds a semantic ID and label to a struct or enum. Representation and behavior remain application-defined.",
    points: [
      "Struct or enum",
      "ID and label only",
      "Always declared in value.rs",
    ],
    file: "src/domain/bike_rental/rental_fleet/bicycle/condition/value.rs",
    authored: `#[derive(ValueObject, Clone, Copy, Debug)]
#[domain(id = "bicycle-condition", label = "Bicycle condition")]
pub enum BicycleCondition {
    Serviceable,
    MaintenanceRequired,
}`,
    generated: `impl ValueObject for BicycleCondition {
    const DESCRIPTOR: ValueObjectDescriptor =
        ValueObjectDescriptor {
            id: ValueObjectId("bicycle-condition"),
            label: "Bicycle condition",
        };
}`,
  },
  {
    name: "DomainService",
    family: "derive",
    headline: "Put cross-object behavior in context.",
    description:
      "The derive names a stateless service marker; DomainServiceDefinition binds it to one bounded context.",
    points: [
      "Non-generic unit struct",
      "Context relationship is explicit",
      "Actions and policies are ordinary traits",
    ],
    file: "src/domain/bike_rental/bicycle_transfer/service.rs",
    authored: `#[derive(DomainService)]
#[domain(id = "bicycle-transfer", label = "Bicycle transfer")]
pub struct BicycleTransfer;

impl DomainServiceDefinition for BicycleTransfer {
    type Context = BikeRental;
}`,
    generated: `impl DomainServiceType for BicycleTransfer {
    const DESCRIPTOR: DomainServiceDescriptor =
        DomainServiceDescriptor {
            id: /* context-scoped service ID */,
            label: "Bicycle transfer",
        };
}`,
  },
  {
    name: "Command",
    family: "derive",
    headline: "Define a bounded-context command.",
    description:
      "The payload declares its context and carries every required aggregate ID. The derive generates metadata and a JSON codec.",
    points: [
      "Schema version 1 by default",
      "Explicit bounded context",
      "Handler loads participating aggregates",
    ],
    file: "src/domain/bike_rental/rental_fleet/rent_bicycle/command.rs",
    authored: `#[derive(Command, Clone, Debug)]
#[domain(
    id = "rent-bicycle",
    label = "Rent bicycle",
    context = BikeRental,
    schema_version = 2
)]
pub struct RentBicycle {
    pub fleet_id: FleetId,
    pub bicycle_id: BicycleId,
}`,
    generated: `impl Command for RentBicycle {
    type Context = BikeRental;
    const LOCAL_ID: &str = "rent-bicycle";
    const LABEL: &str = "Rent bicycle";
    const SCHEMA_VERSION: u32 = 2;
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
    headline: "Describe a recorded domain fact.",
    description:
      "Declares event metadata. The aggregate's event set supplies membership, JSON encoding, and replay dispatch.",
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
    headline: "Define a business rejection.",
    description:
      "Declares a rejection code, message, and structured details, with generated JSON encoding.",
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
    message = "The requested bicycle cannot currently be rented."
)]
pub struct BicycleUnavailable {
    pub bicycle_id: BicycleId,
}`,
    generated: `impl DomainError for BicycleUnavailable {
    const LOCAL_ID: &str = "bicycle-unavailable";
    const LABEL: &str = "Bicycle unavailable";
    const CODE: &str = "BICYCLE_UNAVAILABLE";
    const MESSAGE: &str = "The requested bicycle cannot currently be rented.";
    const FIELDS: &[FieldDescriptor] = /* … */;
}

impl JsonErrorPayload for BicycleUnavailable { /* … */ }`,
  },
  {
    name: "PolicyOutcome",
    family: "derive",
    headline: "Keep outcomes closed and exhaustive.",
    description:
      "The enum remains ordinary Rust while each business outcome receives a stable local ID and label.",
    points: [
      "Non-empty enum",
      "Ordered outcome metadata",
      "Arbitrary Rust payloads",
    ],
    file: "src/domain/bike_rental/rental_fleet/bicycle/assess_rental_eligibility/outcome.rs",
    authored: `#[derive(PolicyOutcome, Clone, Copy, Debug)]
pub enum RentalEligibilityOutcome {
    #[outcome(id = "eligible", label = "Eligible")]
    Eligible,
    #[outcome(id = "unavailable-status", label = "Unavailable status")]
    UnavailableStatus,
    #[outcome(
        id = "maintenance-required",
        label = "Maintenance required"
    )]
    MaintenanceRequired,
}`,
    generated: `impl PolicyOutcomeType for RentalEligibilityOutcome {
    const OUTCOMES: &[PolicyOutcomeDescriptor] = &[
        PolicyOutcomeDescriptor {
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
    headline: "Declare lifecycle states.",
    description:
      "The entity's state enum supplies metadata and an initial state for typed transition evaluation.",
    points: [
      "Fieldless enum",
      "Explicit initial state",
      "LifecycleState implementation",
    ],
    file: "src/domain/bike_rental/rental_fleet/bicycle/rental_status/lifecycle.rs",
    authored: `#[derive(EntityLifecycle, Clone, Copy, Eq, PartialEq)]
#[domain(id = "rental-status", label = "Bicycle rental status")]
#[lifecycle(initial = Available)]
pub enum BicycleStatus {
    #[state(id = "available", label = "Available")]
    Available,
    #[state(id = "rented", label = "Rented")]
    Rented,
    #[state(id = "retired", label = "Retired")]
    Retired,
}`,
    generated: `impl EntityLifecycleType for BicycleStatus {
    const DESCRIPTOR: EntityLifecycleDescriptor =
        EntityLifecycleDescriptor {
            id: EntityLifecycleId("rental-status"),
            label: "Bicycle rental status",
            initial: /* available state ID */,
            states: &[/* available, rented, retired */],
        };
}

impl LifecycleState for BicycleStatus {
    const INITIAL: Self = Self::Available;
    fn state_id(self) -> EntityLifecycleStateId { /* … */ }
}`,
  },
  {
    name: "StateTransition",
    family: "derive",
    headline: "Declare legal edges for each trigger.",
    description:
      "A transition enum links stable trigger metadata to one or more source and target lifecycle state pairs.",
    points: [
      "Fieldless enum",
      "One or more typed edges",
      "Pure rejection for illegal state",
    ],
    file: "src/domain/bike_rental/rental_fleet/bicycle/rental_status/transition.rs",
    authored: `#[derive(StateTransition)]
#[transition(state = BicycleStatus)]
pub enum BicycleRentalTransition {
    #[transition(id = "rent", label = "Rent")]
    #[edge(from = Available, to = Rented)]
    Rent,
    #[transition(id = "return", label = "Return")]
    #[edge(from = Rented, to = Available)]
    Return,
    #[transition(id = "retire", label = "Retire")]
    #[edge(from = Available, to = Retired)]
    #[edge(from = Rented, to = Retired)]
    Retire,
}`,
    generated: `impl StateTransition for BicycleRentalTransition {
    type State = BicycleStatus;
    const DESCRIPTORS: &'static [StateTransitionDescriptor<Self::State>] =
        &[/* rent, return, and retire logical transitions with edges */];

    fn descriptor(&self) -> &'static StateTransitionDescriptor<Self::State> {
        /* one logical descriptor per variant */
    }
}`,
  },
  {
    name: "domain_action",
    family: "behavior",
    headline: "Keep actions as ordinary traits.",
    description:
      "The macro preserves the trait and adds only owner-independent metadata. The checker links its implementation through nesting.",
    points: [
      "Preserves the authored trait",
      "Application-defined signatures",
      "Checked implementation owner",
    ],
    file: "src/domain/bike_rental/rental_fleet/rent_bicycle/action.rs",
    authored: `#[domain_action(id = "rent-bicycle", label = "Rent bicycle")]
pub trait RentBicycleAction {
    fn rent_bicycle(
        &mut self,
        input: BicycleId,
    ) -> Result<(), BicycleUnavailable>;
}`,
    generated: `pub trait RentBicycleAction {
    fn rent_bicycle(
        &mut self,
        input: BicycleId,
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
      "Separate from HTTP query registration",
    ],
    file: "src/domain/bike_rental/rental_fleet/bicycle_availability/query.rs",
    authored: `#[domain_query(
    id = "bicycle-availability",
    label = "Bicycle availability"
)]
pub trait BicycleAvailabilityQuery {
    fn bicycle_availability(
        &self,
        input: &BicycleId,
    ) -> Option<BicycleAvailability>;
}`,
    generated: `pub trait BicycleAvailabilityQuery {
    fn bicycle_availability(
        &self,
        input: &BicycleId,
    ) -> Option<BicycleAvailability>;

    const LOCAL_ID: &str = "bicycle-availability";
    const LABEL: &str = "Bicycle availability";
    const DESCRIPTOR: QueryDescriptor = /* … */;
}`,
  },
  {
    name: "domain_policy",
    family: "behavior",
    headline: "Evaluate domain facts without mutation.",
    description:
      "A policy returns a business outcome from supplied facts. The application is responsible for keeping evaluation pure.",
    points: [
      "Aggregate, entity, value object, or service",
      "Application-defined inputs and outputs",
      "Optional PolicyOutcome enum",
    ],
    file: "src/domain/bike_rental/rental_fleet/bicycle/assess_rental_eligibility/policy.rs",
    authored: `#[domain_policy(
    id = "assess-rental-eligibility",
    label = "Assess rental eligibility"
)]
pub trait RentalEligibilityPolicy {
    fn assess_rental_eligibility(&self) -> RentalEligibilityOutcome;
}`,
    generated: `pub trait RentalEligibilityPolicy {
    fn assess_rental_eligibility(&self) -> RentalEligibilityOutcome;

    const LOCAL_ID: &str = "assess-rental-eligibility";
    const LABEL: &str = "Assess rental eligibility";
    const DESCRIPTOR: PolicyDescriptor = /* … */;
}`,
  },
  {
    name: "domain_invariant",
    family: "behavior",
    headline: "Separate a rule from its evaluation.",
    description:
      "The trait describes a rule for domain state. evaluate.rs implements it for the enclosing aggregate, entity, or value object.",
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
    fn unique_bicycle_identities(
        candidate: &RentalFleet,
    ) -> Option<InvariantViolation>;
}`,
    generated: `pub trait FleetConsistency {
    fn unique_bicycle_identities(
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
    headline: "Link a test to its domain subject.",
    description:
      "Action, policy, invariant, and lifecycle tests remain normal Rust tests with discoverable subject metadata.",
    points: [
      "Authored body stays unchanged",
      "Typed descriptor expression",
      "Ignored discovery companion",
    ],
    file: "src/domain/tests/bike_rental/rental_fleet/fleet_consistency.rs",
    authored: `#[domain_invariant_test(
    <RentalFleetAggregate as FleetConsistency>::DESCRIPTOR
)]
fn exposes_owner_independent_invariant_metadata() {
    // inspect the authored invariant descriptor
}`,
    generated: `#[test]
fn exposes_owner_independent_invariant_metadata() {
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
      "Events and identities follow type relationships",
    ],
    file: "src/domain/model.rs",
    authored: `domain_model! {
    contexts: [BikeRental],
    aggregates: [RentalFleetAggregate],
    entities: [RentalFleet, Bicycle],
    value_objects: [BicycleCondition, RegistrationNumber, BicycleTransferRejectionReason],
    services: [BicycleTransfer],
    errors: [BicycleUnavailable, BicycleNotRented, BicycleAlreadyInFleet, BicycleCannotBeRetired, BicycleTransferRejected],
}`,
    generated: `try_build(|builder| {
    builder.add_bounded_context(BikeRental::DESCRIPTOR)?;
    builder.add_aggregate_type::<RentalFleetAggregate>()?;
    builder.add_entity_type::<RentalFleet>()?;
    builder.add_entity_type::<Bicycle>()?;
    builder.add_value_object_type::<BicycleCondition>()?;
    builder.add_value_object_type::<RegistrationNumber>()?;
    builder.add_value_object_type::<BicycleTransferRejectionReason>()?;
    builder.add_domain_service(BicycleTransfer::DESCRIPTOR)?;
    builder.add_domain_error(BicycleUnavailable::DESCRIPTOR)?;
    builder.add_domain_error(BicycleNotRented::DESCRIPTOR)?;
    builder.add_domain_error(BicycleAlreadyInFleet::DESCRIPTOR)?;
    builder.add_domain_error(BicycleCannotBeRetired::DESCRIPTOR)?;
    builder.add_domain_error(BicycleTransferRejected::DESCRIPTOR)?;
    Ok(())
})`,
  },
]
