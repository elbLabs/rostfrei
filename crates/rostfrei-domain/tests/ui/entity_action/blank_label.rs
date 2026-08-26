use rostfrei_domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "blank", label = "  ")]
    fn blank(&self);
}

fn main() {}
