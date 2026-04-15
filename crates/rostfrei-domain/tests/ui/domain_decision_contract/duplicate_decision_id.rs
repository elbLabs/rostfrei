use domain::domain_decisions;

struct Owner;

#[domain_decisions(aggregate)]
impl Owner {
    #[decision(id = "decide", label = "First")]
    fn first() -> Result<(), ()> {
        Ok(())
    }

    #[decision(id = "decide", label = "Second")]
    fn second() -> Result<(), ()> {
        Ok(())
    }
}

fn main() {}
