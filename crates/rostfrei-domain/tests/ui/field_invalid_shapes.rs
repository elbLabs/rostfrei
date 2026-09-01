use domain::{DomainIdentity, Entity};

#[derive(DomainIdentity)]
struct Id(u64);

struct Custom;

#[derive(Entity)]
#[domain(id = "custom", label = "Custom")]
struct UntaggedCustom {
    #[domain(identity)]
    id: Id,
    value: Custom,
}

impl domain::EntityDefinition for UntaggedCustom {
    type Owner = Owner;
    type Identity = Id;
}

#[derive(Entity)]
#[domain(id = "reference", label = "Reference")]
struct Reference {
    #[domain(identity)]
    id: Id,
    value: &'static str,
}

impl domain::EntityDefinition for Reference {
    type Owner = Owner;
    type Identity = Id;
}

#[derive(Entity)]
#[domain(id = "array", label = "Array")]
struct Array {
    #[domain(identity)]
    id: Id,
    value: [u8; 4],
}

impl domain::EntityDefinition for Array {
    type Owner = Owner;
    type Identity = Id;
}

#[derive(Entity)]
#[domain(id = "tuple", label = "Tuple")]
struct Tuple {
    #[domain(identity)]
    id: Id,
    value: (u8, u8),
}

impl domain::EntityDefinition for Tuple {
    type Owner = Owner;
    type Identity = Id;
}

#[derive(Entity)]
#[domain(id = "map", label = "Map")]
struct Map {
    #[domain(identity)]
    id: Id,
    value: std::collections::HashMap<String, String>,
}

impl domain::EntityDefinition for Map {
    type Owner = Owner;
    type Identity = Id;
}

#[derive(Entity)]
#[domain(id = "alias", label = "Alias")]
struct NonCanonical {
    #[domain(identity)]
    id: Id,
    value: collections::Vec<u8>,
}

impl domain::EntityDefinition for NonCanonical {
    type Owner = Owner;
    type Identity = Id;
}

#[derive(Entity)]
#[domain(id = "malformed", label = "Malformed")]
struct MalformedWrapper {
    #[domain(identity)]
    id: Id,
    value: Option<u8, u16>,
}

impl domain::EntityDefinition for MalformedWrapper {
    type Owner = Owner;
    type Identity = Id;
}

fn main() {}
