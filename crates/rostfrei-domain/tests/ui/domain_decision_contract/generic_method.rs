use domain::domain_decisions;

struct Owner;
struct Group;
struct Outcome;

#[domain_decisions(aggregate, group = Group)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide<T>() -> Outcome {
        Outcome
    }
}

fn main() {}
