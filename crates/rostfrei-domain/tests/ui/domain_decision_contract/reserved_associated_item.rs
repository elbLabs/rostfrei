use rostfrei_domain::domain_decisions;

struct Input;
struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "reserved", label = "Reserved")]
    fn __DOMAIN_DECISIONS_TRAIT_REQUIRES_DOMAIN_DECISIONS_ATTRIBUTE(input: Input) -> Output;
}

fn main() {}
