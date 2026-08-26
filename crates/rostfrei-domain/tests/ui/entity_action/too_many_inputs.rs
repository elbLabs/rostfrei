use rostfrei_domain::domain_actions;

#[domain_actions(entity)]
trait Actions {
    #[action(id = "change", label = "Change")]
    fn change(&self, input: u8, other: u8);
}

fn main() {}
