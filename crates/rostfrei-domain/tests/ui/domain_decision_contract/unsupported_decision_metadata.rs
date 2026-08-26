use rostfrei_domain::domain_decisions;

struct Input;
struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "decide", label = "Decide", other = "value")]
    fn decide(input: Input) -> Output;
}

fn main() {}
