use rostfrei_domain::{ActionReference, EntityActionOwnerType, domain_actions};

#[domain_actions(entity)]
trait Actions {
    #[action(id = "inspect", label = "Inspect")]
    fn inspect(&self);
}

fn missing_implementation<Owner: EntityActionOwnerType>() {
    let _: ActionReference<Owner> = <Owner as Actions>::__DOMAIN_ACTION_REFERENCE_INSPECT;
}

fn main() {}
