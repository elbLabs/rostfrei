use rostfrei_domain::{ActionId, ActionReference, domain_actions};

#[domain_actions(domain_service)]
pub trait Actions {
    #[action(id = "dispatch-request", label = "Dispatch request")]
    fn execute();

    #[action(id = "2fa-start", label = "Start 2FA")]
    fn authenticate();

    #[action(id = "descriptor", label = "Descriptor")]
    fn describe();

    #[action(id = "action-owner-id", label = "Action owner ID")]
    fn identify();
}

fn references<Owner: Actions>() {
    let dispatch: ActionReference<Owner> =
        <Owner as Actions>::__DOMAIN_ACTION_REFERENCE_DISPATCH_REQUEST;
    let _: &'static str = dispatch.local_id();
    let _: ActionId = dispatch.id();

    let numeric: ActionReference<Owner> = <Owner as Actions>::__DOMAIN_ACTION_REFERENCE__2FA_START;
    let _: &'static str = numeric.local_id();
    let _: ActionId = numeric.id();

    let descriptor: ActionReference<Owner> =
        <Owner as Actions>::__DOMAIN_ACTION_REFERENCE_DESCRIPTOR;
    let _: &'static str = descriptor.local_id();

    let owner_id: ActionReference<Owner> =
        <Owner as Actions>::__DOMAIN_ACTION_REFERENCE_ACTION_OWNER_ID;
    let _: &'static str = owner_id.local_id();
}

fn main() {}
