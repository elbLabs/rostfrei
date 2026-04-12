use domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    const __DOMAIN_ACTIONS_TRAIT_REQUIRES_DOMAIN_ACTIONS_ATTRIBUTE: ();
}

fn main() {}
