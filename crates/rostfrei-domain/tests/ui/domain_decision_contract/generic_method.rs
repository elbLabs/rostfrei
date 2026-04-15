use domain::domain_decisions;

struct Owner;

#[domain_decisions(aggregate)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide<T>() -> Result<(), ()> {
        Ok(())
    }
}

fn main() {}
