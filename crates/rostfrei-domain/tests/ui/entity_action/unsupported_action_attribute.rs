use domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "unsupported", label = "Unsupported", other = "value")]
    fn unsupported(&self);
}

fn main() {}
