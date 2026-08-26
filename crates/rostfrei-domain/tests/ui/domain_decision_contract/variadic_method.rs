use domain::domain_decisions;

struct Input;
struct Output;

#[domain_decisions(entity)]
trait Decisions {
    #[decision(id = "variadic", label = "Variadic")]
    fn variadic(input: Input, _: ...) -> Output;
}

fn main() {}
