use domain::domain_decisions;

struct Owner;
struct Group;
struct Outcome;

#[domain_decisions(entity, group = Group)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide(&self) -> Outcome {
        Outcome
    }
}

fn main() {}
