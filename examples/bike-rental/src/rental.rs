use rostfrei::{
    Aggregate, BoundedContext, DomainCommand, DomainError, DomainEvent, DomainIdentity, Entity,
    ValueObject, domain_actions, domain_decisions, domain_queries,
};
use serde::{Deserialize, Serialize};

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
    events = [RentalFleetImported, BicycleRented]
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

    pub fn fleet_id(&self) -> &FleetId {
        &self.fleet_id
    }

    pub(crate) fn apply_rental(&mut self, bicycle_id: &BicycleId) {
        if let Some(bicycle) = self
            .bicycles
            .iter_mut()
            .find(|bicycle| bicycle.bicycle_id() == bicycle_id)
        {
            bicycle.mark_rented(BicycleStatus::Rented);
        }
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

    pub fn condition(&self) -> BicycleCondition {
        self.condition
    }
}

#[derive(
    DomainIdentity, Clone, Debug, Deserialize, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize,
)]
#[domain(owner = RentalFleet)]
#[serde(try_from = "String")]
pub struct FleetId(String);

impl FleetId {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        non_empty(value).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for FleetId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("fleet ID must be non-empty and trimmed")
    }
}

#[derive(
    DomainIdentity, Clone, Debug, Deserialize, Eq, Hash, PartialEq, Ord, PartialOrd, Serialize,
)]
#[domain(owner = Bicycle)]
#[serde(try_from = "String")]
pub struct BicycleId(String);

impl BicycleId {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        non_empty(value).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for BicycleId {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("bicycle ID must be non-empty and trimmed")
    }
}

#[derive(ValueObject, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-status", label = "Bicycle status", owner = RentalFleetAggregate)]
#[serde(rename_all = "kebab-case")]
pub enum BicycleStatus {
    Available,
    Rented,
}

#[derive(ValueObject, Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(
    id = "bicycle-condition",
    label = "Bicycle condition",
    owner = RentalFleetAggregate
)]
#[serde(rename_all = "kebab-case")]
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
    pub(crate) status: BicycleStatus,
    #[domain(value_object)]
    pub(crate) condition: BicycleCondition,
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

#[derive(ValueObject, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(
    id = "imported-bicycle",
    label = "Imported bicycle",
    owner = RentalFleetAggregate
)]
pub struct ImportedBicycle {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
    #[domain(value_object)]
    pub status: BicycleStatus,
    #[domain(value_object)]
    pub condition: BicycleCondition,
}

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "rental-fleet-imported", label = "Rental fleet imported")]
pub struct RentalFleetImported {
    #[domain(identity)]
    pub fleet_id: FleetId,
    #[domain(value_object)]
    pub bicycles: Vec<ImportedBicycle>,
}

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
        let event = decide_rental(root, &input)?;
        root.apply_rental(&event.bicycle_id);
        Ok(event)
    }
}

pub(crate) fn decide_rental(
    root: &RentalFleet,
    input: &RentBicycle,
) -> Result<BicycleRented, BicycleUnavailable> {
    let bicycle = root
        .bicycles
        .iter()
        .find(|bicycle| bicycle.bicycle_id == input.bicycle_id)
        .ok_or_else(|| BicycleUnavailable {
            bicycle_id: input.bicycle_id.clone(),
        })?;
    let eligibility = RentalEligibilityInput {
        status: bicycle.status,
        condition: bicycle.condition,
    };
    if RentalFleetAggregate::assess_rental_eligibility(eligibility) != RentalEligibility::Eligible {
        return Err(BicycleUnavailable {
            bicycle_id: input.bicycle_id.clone(),
        });
    }

    Ok(BicycleRented {
        fleet_id: root.fleet_id.clone(),
        bicycle_id: input.bicycle_id.clone(),
    })
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
