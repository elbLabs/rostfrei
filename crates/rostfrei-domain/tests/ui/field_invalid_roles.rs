use domain::{DomainIdentity, Entity, ValueObject};

#[derive(DomainIdentity)]
struct Id(u64);

struct Other;

#[derive(Entity)]
#[domain(id = "bad", label = "Bad")]
struct Unsupported {
    id: Id,
    #[domain(owns)]
    value: String,
}

impl domain::EntityDefinition for Unsupported {
    type Owner = Owner;
    type Identity = Id;

    fn identity(&self) -> &Self::Identity {
        &self.id
    }
}

#[derive(ValueObject)]
#[domain(id = "bad", label = "Bad")]
struct EntityInValueObject(#[domain(entity)] Other);

fn main() {}
rostfrei_domain_macros::__install_test_macro_support!();
