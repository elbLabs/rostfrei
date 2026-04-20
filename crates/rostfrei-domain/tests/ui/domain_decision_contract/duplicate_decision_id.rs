use domain::domain_decisions;

struct Owner;
struct Group;
struct Outcome;

#[domain_decisions(aggregate, group = Group)]
impl Owner {
    #[decision(id = "decide", label = "First")]
    fn first() -> Outcome {
        Outcome
    }

    #[decision(id = "decide", label = "Second")]
    fn second() -> Outcome {
        Outcome
    }
}

fn main() {}
