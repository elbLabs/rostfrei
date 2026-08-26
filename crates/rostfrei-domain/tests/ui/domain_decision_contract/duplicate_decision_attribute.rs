use domain::domain_decisions;

struct Input;
struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "first", label = "First")]
    #[decision(id = "second", label = "Second")]
    fn decide(input: Input) -> Output;
}

fn main() {}
