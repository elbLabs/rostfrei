use rostfrei_domain::domain_decisions;

struct Input;
struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "decide", id = "other", label = "Decide")]
    fn decide(input: Input) -> Output;
}

fn main() {}
