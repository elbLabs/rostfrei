use domain::domain_decisions;

struct Owner;
struct Group;

#[domain_decisions(aggregate, group = Group)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide() {}
}

fn main() {}
