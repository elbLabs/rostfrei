use domain::{
    Aggregate, BoundedContext, DomainEvent, DomainIdentity, DomainService, Entity, domain_actions,
};

#[derive(BoundedContext)]
#[domain(id = "services", label = "Services")]
struct Services;

#[derive(BoundedContext)]
#[domain(id = "orders", label = "Orders")]
struct Orders;

#[derive(DomainIdentity)]
#[domain(owner = OrderRoot)]
struct OrderId(u8);

#[derive(Entity)]
#[domain(id = "order-root", label = "Order root", owner = Order)]
struct OrderRoot {
    #[domain(identity)]
    id: OrderId,
}

#[derive(Aggregate)]
#[domain(id = "order", label = "Order")]
struct Order;

impl domain::AggregateDefinition for Order {
    type Context = Orders;
    type Root = OrderRoot;
    type Event = OrderEvents;
}

#[derive(domain::AggregateEvents)]
enum OrderEvents {
    Event0(Placed),
}

#[derive(DomainEvent)]
#[domain(id = "placed", label = "Placed")]
pub struct Placed;

#[domain_actions(domain_service)]
pub trait Actions {
    #[action(id = "execute", label = "Execute")]
    fn execute() -> Placed;
}

#[derive(DomainService)]
#[domain(id = "service", label = "Service", context = Services, actions = [Actions])]
struct Service;

impl Actions for Service {
    fn execute() -> Placed {
        Placed
    }
}

fn main() {}
