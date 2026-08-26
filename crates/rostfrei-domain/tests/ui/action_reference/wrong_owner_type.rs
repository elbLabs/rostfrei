use rostfrei_domain::{ActionOwnerType, ActionReference, domain_actions};

#[domain_actions(entity)]
trait Actions {
    #[action(id = "inspect", label = "Inspect")]
    fn inspect(&self);
}

fn wrong_owner<Owner, Other>()
where
    Owner: Actions,
    Other: ActionOwnerType,
{
    let _: ActionReference<Other> = <Owner as Actions>::__DOMAIN_ACTION_REFERENCE_INSPECT;
}

fn main() {}
