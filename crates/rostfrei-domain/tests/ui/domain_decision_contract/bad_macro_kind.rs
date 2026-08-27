use domain::domain_decisions;

struct Owner;

#[domain_decisions(domain_service)]
impl Owner {
    #[decision(id = "decide", label = "Decide")]
    fn decide() -> Result<(), ()> {
        Ok(())
    }
}

fn main() {}
