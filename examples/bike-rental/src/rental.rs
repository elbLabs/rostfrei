use rostfrei::{
    Aggregate, AggregateInstance, BoundedContext, DecisionOutcome, DomainCommand, DomainError,
    DomainEvent, DomainIdentity, Entity, StreamAggregateId, ValueObject, domain_actions,
    domain_decisions, domain_queries,
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
    actions = [RentalFleetActionContract],
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

#[derive(DomainCommand, Clone, Debug, Eq, PartialEq)]
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
    message = "The requested bicycle cannot currently be rented.",
    json
)]
pub struct BicycleUnavailable {
    #[domain(identity)]
    pub bicycle_id: BicycleId,
}

pub(crate) struct RentalEligibilityDecisions;

#[domain_decisions(aggregate, group = RentalEligibilityDecisions)]
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
    #[action(
        id = "import-rental-fleet",
        label = "Import rental fleet",
        raises = [RentalFleetImported]
    )]
    fn import_rental_fleet(&mut self, input: ImportRentalFleetInput);

    #[action(
        id = "rent-bicycle",
        label = "Rent bicycle",
        raises = [BicycleRented]
    )]
    fn rent_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleUnavailable>;
}

impl RentalFleetActions for AggregateInstance<RentalFleetAggregate> {
    fn import_rental_fleet(&mut self, input: ImportRentalFleetInput) {
        self.raise(RentalFleetImported {
            fleet_id: self.state().fleet_id.clone(),
            bicycles: input.bicycles,
        });
    }

    fn rent_bicycle(&mut self, input: BicycleId) -> Result<(), BicycleUnavailable> {
        let event = {
            let root = self.state();
            let bicycle = root
                .bicycles
                .iter()
                .find(|bicycle| bicycle.bicycle_id == input)
                .ok_or_else(|| BicycleUnavailable {
                    bicycle_id: input.clone(),
                })?;
            match RentalFleetAggregate::assess_rental_eligibility(bicycle.status, bicycle.condition)
            {
                RentalEligibilityOutcome::Eligible => BicycleRented {
                    fleet_id: root.fleet_id.clone(),
                    bicycle_id: input.clone(),
                },
                RentalEligibilityOutcome::AlreadyRented
                | RentalEligibilityOutcome::MaintenanceRequired => {
                    return Err(BicycleUnavailable {
                        bicycle_id: input.clone(),
                    });
                }
            }
        };

        self.raise(event);
        Ok(())
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
