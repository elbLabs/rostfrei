use domain::domain_decisions;

struct Owner;

#[domain_decisions(entity)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide(&self) -> Result<(), ()> {
        Ok(())
    }
}

fn main() {}
