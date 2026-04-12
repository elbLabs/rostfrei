use rostfrei_domain::domain_decisions;

struct Input;
struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "accept", label = "Accept")]
    fn __DOMAIN_DECISION_REFERENCE_ACCEPT(input: Input) -> Output;
}

fn main() {}
