use rostfrei_domain::domain_actions;

#[domain_actions(value_object)]
trait Actions {
    #[action(id = "replace", label = "Replace")]
    fn replace(self, input: String, extra: String) -> Self;
}

fn main() {}
