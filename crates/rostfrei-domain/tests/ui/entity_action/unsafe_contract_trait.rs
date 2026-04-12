use rostfrei_domain::domain_actions;

#[domain_actions(entity)]
unsafe trait Actions {
    #[action(id = "rename", label = "Rename")]
    fn rename(&self);
}

fn main() {}
