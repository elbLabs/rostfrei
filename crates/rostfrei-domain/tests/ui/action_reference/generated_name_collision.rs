use rostfrei_domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "inspect", label = "Inspect")]
    fn r#__DOMAIN_ACTION_REFERENCE_INSPECT(&self);
}

fn main() {}
