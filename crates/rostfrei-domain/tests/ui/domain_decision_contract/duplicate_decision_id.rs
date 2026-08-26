use domain::domain_decisions;

struct Input;
struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "decide", label = "First")]
    fn first(input: Input) -> Output;

    #[decision(id = "decide", label = "Second")]
    fn second(input: Input) -> Output;
}

fn main() {}
