#[derive(DomainService)]
#[domain(id = "fleet-planning", label = "Fleet planning")]
pub struct FleetPlanning;

impl DomainServiceDefinition for FleetPlanning {
    type Context = BikeRental;
}
