use domain::domain_decisions;

struct Input;
struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "Not-Kebab", label = "Invalid")]
    fn invalid(input: Input) -> Output;
}

fn main() {}
