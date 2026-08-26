use rostfrei_domain::domain_actions;

#[domain_actions(value_object)]
trait ValueObjectActions {
    #[action(id = "normalize", label = "Normalize")]
    fn normalize(self) -> Self;
}

fn main() {}
