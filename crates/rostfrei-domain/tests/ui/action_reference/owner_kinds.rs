use domain::{ActionReference, domain_actions};

pub struct Root;

#[domain_actions(aggregate)]
pub trait AggregateActions {
    #[action(id = "change", label = "Change")]
    fn apply(root: &mut Root);
}

#[domain_actions(entity)]
trait EntityActions {
    #[action(id = "inspect", label = "Inspect")]
    fn inspect(&self);
}

#[domain_actions(domain_service)]
pub trait DomainServiceActions {
    #[action(id = "dispatch", label = "Dispatch")]
    fn execute();
}

fn aggregate_reference<Owner: AggregateActions>() {
    let _: ActionReference<Owner> = <Owner as AggregateActions>::__DOMAIN_ACTION_REFERENCE_CHANGE;
}

fn entity_reference<Owner: EntityActions>() {
    let _: ActionReference<Owner> = <Owner as EntityActions>::__DOMAIN_ACTION_REFERENCE_INSPECT;
}

fn domain_service_reference<Owner: DomainServiceActions>() {
    let _: ActionReference<Owner> =
        <Owner as DomainServiceActions>::__DOMAIN_ACTION_REFERENCE_DISPATCH;
}

fn main() {}
