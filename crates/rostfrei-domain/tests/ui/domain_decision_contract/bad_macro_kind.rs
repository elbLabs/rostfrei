use domain::domain_decisions;

struct Owner;
struct Group;
struct Outcome;

#[domain_decisions(domain_service, group = Group)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide() -> Outcome {
        Outcome
    }
}

fn main() {}
