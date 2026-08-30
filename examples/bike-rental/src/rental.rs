use std::convert::Infallible;

use rostfrei::{
    Aggregate, AggregateInstance, BoundedContext, Command, DomainError, DomainEvent,
    DomainIdentity, Entity, ValueObject, domain_actions, domain_decisions, domain_queries,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(BoundedContext)]
#[domain(id = "bike-rental", label = "Bike Rental")]
pub struct BikeRental;

#[derive(Aggregate)]
#[domain(
    id = "rental-fleet",
    label = "Rental fleet",
    context = BikeRental,
    root = RentalFleet,
    actions = [RentalFleetActionContract],
    decisions,
    events = [RentalFleetImported, BicycleAdded, BicycleRented, BicycleReturned]
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
    pub const fn new(fleet_id: FleetId, bicycles: Vec<Bicycle>) -> Self {
        Self { fleet_id, bicycles }
    }

    pub fn bicycles(&self) -> &[Bicycle] {
        &self.bicycles
    }

    pub const fn fleet_id(&self) -> &FleetId {
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

    pub(crate) fn apply_return(&mut self, bicycle_id: &BicycleId) {
        if let Some(bicycle) = self
            .bicycles
            .iter_mut()
            .find(|bicycle| bicycle.bicycle_id() == bicycle_id)
        {
            bicycle.mark_available();
        }
    }

    pub(crate) fn apply_addition(&mut self, event: &BicycleAdded) {
        self.bicycles.push(Bicycle::new(
            event.bicycle_id.clone(),
            BicycleStatus::Available,
            event.condition,
        ));
    }
}

#[derive(Entity, Debug)]
#[allow(clippy::struct_field_names)]
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
    pub const fn new(
        bicycle_id: BicycleId,
        status: BicycleStatus,
        condition: BicycleCondition,
    ) -> Self {
        Self {
            bicycle_id,
            status,
            condition,
        }
    }

    pub const fn bicycle_id(&self) -> &BicycleId {
        &self.bicycle_id
    }

    pub const fn status(&self) -> BicycleStatus {
        self.status
    }

    pub const fn condition(&self) -> BicycleCondition {
        self.condition
    }

    fn mark_available(&mut self) {
        self.status = BicycleStatus::Available;
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

impl From<&StreamAggregateId> for FleetId {
    fn from(value: &StreamAggregateId) -> Self {
        Self(value.as_str().to_owned())
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

#[derive(DecisionOutcome, Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RentalEligibilityOutcome {
    #[outcome(id = "eligible", label = "Eligible")]
    Eligible,
    #[outcome(id = "already-rented", label = "Already rented")]
    AlreadyRented,
    #[outcome(id = "maintenance-required", label = "Maintenance required")]
    MaintenanceRequired,
}

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "rent-bicycle",
    label = "Rent bicycle",
    owner = RentalFleetAggregate,
    rejection = BicycleUnavailable,
    json,
    runtime
)]
pub struct RentBicycle {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "return-bicycle",
    label = "Return bicycle",
    owner = RentalFleetAggregate,
    rejection = BicycleNotRented,
    json
)]
pub struct ReturnBicycle {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

#[derive(Command, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "add-bicycle",
    label = "Add bicycle",
    owner = RentalFleetAggregate,
    json
)]
pub struct AddBicycle;

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

#[derive(ValueObject, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "import-rental-fleet-input",
    label = "Import rental fleet input",
    owner = RentalFleetAggregate
)]
pub struct ImportRentalFleetInput {
    #[domain(value_object)]
    bicycles: Vec<ImportedBicycle>,
}

impl ImportRentalFleetInput {
    pub const fn new(bicycles: Vec<ImportedBicycle>) -> Self {
        Self { bicycles }
    }
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
#[domain(id = "bicycle-added", label = "Bicycle added")]
pub struct BicycleAdded {
    #[domain(identity)]
    pub fleet_id: FleetId,
    #[domain(identity)]
    pub bicycle_id: BicycleId,
    #[domain(value_object)]
    pub condition: BicycleCondition,
}

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-rented", label = "Bicycle rented")]
pub struct BicycleRented {
    #[domain(identity)]
    pub fleet_id: FleetId,
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

#[derive(DomainEvent, Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[domain(id = "bicycle-returned", label = "Bicycle returned")]
pub struct BicycleReturned {
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
    message = "The requested bicycle cannot currently be rented.",
    json
)]
pub struct BicycleUnavailable {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

#[derive(DomainError, Clone, Debug, Eq, PartialEq)]
#[domain(
    id = "bicycle-not-rented",
    label = "Bicycle not rented",
    owner = RentalFleetAggregate,
    code = "BICYCLE_NOT_RENTED",
    message = "The requested bicycle is not currently rented.",
    json
)]
pub struct BicycleNotRented {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

#[domain_decisions(aggregate)]
impl RentalFleetAggregate {
    #[decision(id = "assess-rental-eligibility", label = "Assess rental eligibility")]
    pub(crate) fn assess_rental_eligibility(
        status: BicycleStatus,
        condition: BicycleCondition,
    ) -> RentalEligibilityOutcome {
        if status == BicycleStatus::Rented {
            return RentalEligibilityOutcome::AlreadyRented;
        }
        if condition == BicycleCondition::MaintenanceRequired {
            return RentalEligibilityOutcome::MaintenanceRequired;
        }
        RentalEligibilityOutcome::Eligible
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

#[domain_actions(aggregate(instance = RentalFleetActions))]
pub trait RentalFleetActionContract {
    #[action(id = "rent-bicycle", label = "Rent bicycle")]
    fn rent_bicycle(
        root: &mut RentalFleet,
        input: RentBicycle,
    ) -> Result<BicycleRented, BicycleUnavailable>;

    #[action(id = "return-bicycle", label = "Return bicycle")]
    fn return_bicycle(
        root: &mut RentalFleet,
        input: ReturnBicycle,
    ) -> Result<BicycleReturned, BicycleNotRented>;

    #[action(id = "add-bicycle", label = "Add bicycle")]
    fn add_bicycle(root: &mut RentalFleet, input: AddBicycle) -> BicycleAdded;
}

impl RentalFleetActionContract for RentalFleetAggregate {
    fn rent_bicycle(
        root: &mut RentalFleet,
        input: RentBicycle,
    ) -> Result<BicycleRented, BicycleUnavailable> {
        ensure_rental_allowed(root, &input)?;
        Ok(BicycleRented {
            fleet_id: root.fleet_id.clone(),
            bicycle_id: input.bicycle_id,
        })
    }

    fn return_bicycle(
        root: &mut RentalFleet,
        input: ReturnBicycle,
    ) -> Result<BicycleReturned, BicycleNotRented> {
        ensure_return_allowed(root, &input)?;
        Ok(BicycleReturned {
            fleet_id: root.fleet_id.clone(),
            bicycle_id: input.bicycle_id,
        })
    }

    fn add_bicycle(root: &mut RentalFleet, _input: AddBicycle) -> BicycleAdded {
        BicycleAdded {
            fleet_id: root.fleet_id.clone(),
            bicycle_id: allocate_bicycle_id(root),
            condition: BicycleCondition::Serviceable,
        }
    }
}

pub trait RentalFleetActions {
    fn rent_bicycle(&mut self, input: &RentBicycle) -> Result<(), BicycleUnavailable>;

    fn return_bicycle(&mut self, input: &ReturnBicycle) -> Result<(), BicycleNotRented>;

    fn add_bicycle(&mut self, input: &AddBicycle) -> Result<(), Infallible>;
}

impl RentalFleetActions for AggregateInstance<RentalFleetAggregate> {
    fn rent_bicycle(&mut self, input: &RentBicycle) -> Result<(), BicycleUnavailable> {
        ensure_rental_allowed(self.state(), input)?;
        let fleet_id = self.state().fleet_id.clone();
        self.raise(BicycleRented {
            fleet_id,
            bicycle_id: input.bicycle_id.clone(),
        });
        Ok(())
    }

    fn return_bicycle(&mut self, input: &ReturnBicycle) -> Result<(), BicycleNotRented> {
        ensure_return_allowed(self.state(), input)?;
        let fleet_id = self.state().fleet_id.clone();
        self.raise(BicycleReturned {
            fleet_id,
            bicycle_id: input.bicycle_id.clone(),
        });
        Ok(())
    }

    fn add_bicycle(&mut self, _input: &AddBicycle) -> Result<(), Infallible> {
        let fleet_id = self.state().fleet_id.clone();
        let bicycle_id = allocate_bicycle_id(self.state());
        self.raise(BicycleAdded {
            fleet_id,
            bicycle_id,
            condition: BicycleCondition::Serviceable,
        });
        Ok(())
    }
}

fn ensure_rental_allowed(
    root: &RentalFleet,
    input: &RentBicycle,
) -> Result<(), BicycleUnavailable> {
    let bicycle = root
        .bicycles
        .iter()
        .find(|bicycle| bicycle.bicycle_id == input.bicycle_id)
        .ok_or_else(|| BicycleUnavailable {
            bicycle_id: input.bicycle_id.clone(),
        })?;
    RentalFleetAggregate::assess_rental_eligibility(bicycle.status, bicycle.condition).map_err(
        |_| BicycleUnavailable {
            bicycle_id: input.bicycle_id.clone(),
        },
    )?;

    Ok(())
}

fn ensure_return_allowed(
    root: &RentalFleet,
    input: &ReturnBicycle,
) -> Result<(), BicycleNotRented> {
    let rented = root.bicycles.iter().any(|bicycle| {
        bicycle.bicycle_id == input.bicycle_id && bicycle.status == BicycleStatus::Rented
    });
    if !rented {
        return Err(BicycleNotRented {
            bicycle_id: input.bicycle_id.clone(),
        });
    }
    Ok(())
}

fn allocate_bicycle_id(root: &RentalFleet) -> BicycleId {
    let mut sequence = root.bicycles.len();
    loop {
        let seed = format!(
            "rostfrei:bike-rental:bicycle:v1:{}:{sequence}",
            root.fleet_id.as_str()
        );
        let candidate =
            BicycleId::new(Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string())
                .expect("a canonical UUID is a valid bicycle identity");
        if root
            .bicycles
            .iter()
            .all(|bicycle| bicycle.bicycle_id != candidate)
        {
            return candidate;
        }
        sequence = sequence
            .checked_add(1)
            .expect("a fleet cannot contain more bicycles than addressable memory");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rental_eligibility_returns_first_class_outcomes() {
        assert_eq!(
            RentalFleetAggregate::assess_rental_eligibility(
                BicycleStatus::Available,
                BicycleCondition::Serviceable,
            ),
            RentalEligibilityOutcome::Eligible
        );
        assert_eq!(
            RentalFleetAggregate::assess_rental_eligibility(
                BicycleStatus::Rented,
                BicycleCondition::Serviceable,
            ),
            RentalEligibilityOutcome::AlreadyRented
        );
        assert_eq!(
            RentalFleetAggregate::assess_rental_eligibility(
                BicycleStatus::Available,
                BicycleCondition::MaintenanceRequired,
            ),
            RentalEligibilityOutcome::MaintenanceRequired
        );
    }
}
