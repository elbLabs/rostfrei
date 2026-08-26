use rostfrei_domain::domain_decisions;

struct Input;
struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "decide", label = "Decide")]
    unsafe fn decide(input: Input) -> Output;
}

fn main() {}
