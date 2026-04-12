use domain::{ActionReference, domain_actions};

#[domain_actions(entity)]
trait Actions {
    #[action(id = "inspect", label = "Inspect")]
    fn inspect(&self);
}

fn unknown<Owner: Actions>() {
    let _: ActionReference<Owner> = <Owner as Actions>::__DOMAIN_ACTION_REFERENCE_UNKNOWN;
}

fn main() {}
