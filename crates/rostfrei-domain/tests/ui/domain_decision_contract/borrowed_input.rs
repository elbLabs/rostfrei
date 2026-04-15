use domain::domain_decisions;

struct Owner;
struct Input;

#[domain_decisions(aggregate)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide(input: &Input) -> Result<(), ()> {
        Ok(())
    }
}

fn main() {}
