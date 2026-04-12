use domain::domain_actions;

struct Owner;

#[domain_actions(group = Actions)]
impl Owner {}

fn main() {}
