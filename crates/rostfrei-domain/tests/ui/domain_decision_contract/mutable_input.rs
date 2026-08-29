use domain::domain_decisions;

struct Owner;
struct Group;
struct Input;
struct Outcome;

#[domain_decisions(aggregate, group = Group)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide(input: &mut Input) -> Outcome {
        let _ = input;
        Outcome
    }
}

fn main() {}
