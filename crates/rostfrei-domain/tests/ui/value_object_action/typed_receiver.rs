use domain::domain_actions;

#[domain_actions(value_object)]
trait Actions {
    #[action(id = "normalize", label = "Normalize")]
    fn normalize(self: Box<Self>) -> Self;
}

fn main() {}
