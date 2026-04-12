use rostfrei_domain::domain_decisions;

struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "decide", label = "Decide")]
    fn decide() -> Output;
}

fn main() {}
