use domain::{
    Aggregate, BoundedContext, DomainCommand, DomainError, DomainEvent, DomainIdentity, Entity,
    ValueObject, domain_actions, domain_decisions, domain_queries,
};

#[derive(BoundedContext)]
#[domain(id = "bike-rental", label = "Bike Rental")]
pub struct BikeRental;

#[derive(Aggregate)]
#[domain(
    id = "rental-fleet",
    label = "Rental fleet",
    context = BikeRental,
    root = RentalFleet,
    actions = [RentalFleetActions],
    decisions = [RentalEligibilityDecisions],
    events = [BicycleRented]
)]
pub struct RentalFleetAggregate;

#[derive(Entity, Debug)]
#[domain(
    id = "rental-fleet-root",
    label = "Rental fleet",
    owner = RentalFleetAggregate
)]
pub struct RentalFleet {
    #[domain(identity)]
    fleet_id: FleetId,
    #[domain(entity)]
    bicycles: Vec<Bicycle>,
}

impl RentalFleet {
    pub fn new(fleet_id: FleetId, bicycles: Vec<Bicycle>) -> Self {
        Self { fleet_id, bicycles }
    }

    pub fn bicycles(&self) -> &[Bicycle] {
        &self.bicycles
    }
}

#[derive(Entity, Debug)]
#[domain(
    id = "bicycle",
    label = "Bicycle",
    owner = RentalFleetAggregate,
    actions = [BicycleStatusActions]
)]
pub struct Bicycle {
    #[domain(identity)]
    bicycle_id: BicycleId,
    #[domain(value_object)]
    status: BicycleStatus,
    #[domain(value_object)]
    condition: BicycleCondition,
}

impl Bicycle {
    pub fn new(bicycle_id: BicycleId, status: BicycleStatus, condition: BicycleCondition) -> Self {
        Self {
            bicycle_id,
            status,
            condition,
        }
    }

    pub fn bicycle_id(&self) -> &BicycleId {
        &self.bicycle_id
    }

    pub fn status(&self) -> BicycleStatus {
        self.status
    }
}

#[derive(DomainIdentity, Clone, Debug, Eq, Hash, PartialEq, Ord, PartialOrd)]
#[domain(owner = RentalFleet)]
pub struct FleetId(String);

impl FleetId {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        non_empty(value).map(Self)
    }
}

#[derive(DomainIdentity, Clone, Debug, Eq, Hash, PartialEq, Ord, PartialOrd)]
#[domain(owner = Bicycle)]
pub struct BicycleId(String);

impl BicycleId {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        non_empty(value).map(Self)
    }
}

#[derive(ValueObject, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(id = "bicycle-status", label = "Bicycle status", owner = RentalFleetAggregate)]
pub enum BicycleStatus {
    Available,
    Rented,
}

#[derive(ValueObject, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-condition",
    label = "Bicycle condition",
    owner = RentalFleetAggregate
)]
pub enum BicycleCondition {
    Serviceable,
    MaintenanceRequired,
}

#[derive(ValueObject, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-availability",
    label = "Bicycle availability",
    owner = RentalFleetAggregate
)]
pub enum BicycleAvailability {
    Available,
    Unavailable,
}

#[derive(ValueObject, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(
    id = "rental-eligibility-input",
    label = "Rental eligibility input",
    owner = RentalFleetAggregate
)]
pub(crate) struct RentalEligibilityInput {
    #[domain(value_object)]
    status: BicycleStatus,
    #[domain(value_object)]
    condition: BicycleCondition,
}

#[derive(ValueObject, Clone, Copy, Debug, Eq, PartialEq)]
#[domain(
    id = "rental-eligibility",
    label = "Rental eligibility",
    owner = RentalFleetAggregate
)]
pub(crate) enum RentalEligibility {
    Eligible,
    Ineligible,
}

#[derive(DomainCommand, Clone, Debug, Eq, PartialEq)]
#[domain(id = "rent-bicycle", label = "Rent bicycle", owner = RentalFleetAggregate)]
pub struct RentBicycle {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

#[derive(DomainEvent, Clone, Debug, Eq, PartialEq)]
#[domain(id = "bicycle-rented", label = "Bicycle rented")]
pub struct BicycleRented {
    #[domain(identity)]
    pub fleet_id: FleetId,
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

#[derive(DomainError, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-unavailable",
    label = "Bicycle unavailable",
    owner = RentalFleetAggregate,
    code = "BICYCLE_UNAVAILABLE",
    message = "The requested bicycle cannot currently be rented."
)]
pub struct BicycleUnavailable {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

#[domain_decisions(aggregate)]
pub(crate) trait RentalEligibilityDecisions {
    #[decision(id = "assess-rental-eligibility", label = "Assess rental eligibility")]
    fn assess_rental_eligibility(input: RentalEligibilityInput) -> RentalEligibility;
}

impl RentalEligibilityDecisions for RentalFleetAggregate {
    fn assess_rental_eligibility(input: RentalEligibilityInput) -> RentalEligibility {
        if input.status == BicycleStatus::Available
            && input.condition == BicycleCondition::Serviceable
        {
            RentalEligibility::Eligible
        } else {
            RentalEligibility::Ineligible
        }
    }
}

#[domain_actions(entity)]
pub(crate) trait BicycleStatusActions {
    #[action(id = "mark-rented", label = "Mark rented")]
    fn mark_rented(&mut self, input: BicycleStatus);
}

impl BicycleStatusActions for Bicycle {
    fn mark_rented(&mut self, input: BicycleStatus) {
        self.status = input;
    }
}

#[domain_actions(aggregate)]
pub trait RentalFleetActions {
    #[action(id = "rent-bicycle", label = "Rent bicycle")]
    fn rent_bicycle(
        root: &mut RentalFleet,
        input: RentBicycle,
    ) -> Result<BicycleRented, BicycleUnavailable>;
}

impl RentalFleetActions for RentalFleetAggregate {
    fn rent_bicycle(
        root: &mut RentalFleet,
        input: RentBicycle,
    ) -> Result<BicycleRented, BicycleUnavailable> {
        let bicycle = root
            .bicycles
            .iter_mut()
            .find(|bicycle| bicycle.bicycle_id == input.bicycle_id)
            .ok_or_else(|| BicycleUnavailable {
                bicycle_id: input.bicycle_id.clone(),
            })?;
        let eligibility = RentalEligibilityInput {
            status: bicycle.status,
            condition: bicycle.condition,
        };
        if RentalFleetAggregate::assess_rental_eligibility(eligibility)
            != RentalEligibility::Eligible
        {
            return Err(BicycleUnavailable {
                bicycle_id: input.bicycle_id,
            });
        }

        bicycle.mark_rented(BicycleStatus::Rented);
        Ok(BicycleRented {
            fleet_id: root.fleet_id.clone(),
            bicycle_id: input.bicycle_id,
        })
    }
}

#[domain_queries(group = BicycleAvailabilityQueries)]
impl RentalFleetAggregate {
    #[query(id = "bicycle-availability", label = "Bicycle availability")]
    pub fn bicycle_availability(
        root: &RentalFleet,
        input: &BicycleId,
    ) -> Option<BicycleAvailability> {
        root.bicycles
            .iter()
            .find(|bicycle| bicycle.bicycle_id() == input)
            .map(|bicycle| {
                if bicycle.status == BicycleStatus::Available
                    && bicycle.condition == BicycleCondition::Serviceable
                {
                    BicycleAvailability::Available
                } else {
                    BicycleAvailability::Unavailable
                }
            })
    }
}

fn non_empty(value: impl Into<String>) -> Option<String> {
    let value = value.into();
    (!value.trim().is_empty() && value.trim() == value).then_some(value)
}
