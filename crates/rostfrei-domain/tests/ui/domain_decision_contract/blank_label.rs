use rostfrei_domain::domain_decisions;

struct Input;
struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "blank", label = "  ")]
    fn blank(input: Input) -> Output;
}

fn main() {}
