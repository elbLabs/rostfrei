use domain::domain_decisions;

struct Input;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "decide", label = "Decide")]
    fn decide(input: Input);
}

fn main() {}
