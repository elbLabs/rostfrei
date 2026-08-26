use domain::domain_decisions;

struct Input;
struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "decide")]
    fn decide(input: Input) -> Output;
}

fn main() {}
