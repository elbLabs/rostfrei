use domain::domain_actions;

#[domain_actions(domain_service)]
pub trait DomainServiceActions {
    #[action(id = "dispatch", label = "Dispatch")]
    fn dispatch(input: u8);
}

fn main() {}
