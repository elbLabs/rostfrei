use rostfrei_domain::domain_actions;

struct Root;

#[domain_actions(aggregate)]
trait Actions {
    #[action(id = "change", label = "Change")]
    fn change(root: &mut Root);
}

fn main() {}
