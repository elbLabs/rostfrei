use rostfrei_domain::domain_decisions;

struct Input;
struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(label = "Decide")]
    fn decide(input: Input) -> Output;
}

fn main() {}
