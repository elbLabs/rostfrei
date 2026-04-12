use rostfrei_domain::domain_actions;

#[domain_actions(value_object)]
trait Actions {
    #[action(id = "new", label = "New")]
    fn new() -> Self;
}

fn main() {}
