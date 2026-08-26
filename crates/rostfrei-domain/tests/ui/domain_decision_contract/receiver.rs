use domain::domain_decisions;

struct Input;
struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "decide", label = "Decide")]
    fn decide(&self, input: Input) -> Output;
}

fn main() {}
