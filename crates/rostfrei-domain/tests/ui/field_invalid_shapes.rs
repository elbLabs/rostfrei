use domain::{DomainIdentity, Entity};

#[derive(DomainIdentity)]
struct Id(u64);

struct Custom;

#[derive(Entity)]
#[domain(id = "custom", label = "Custom", owner = Owner)]
struct UntaggedCustom {
    #[domain(identity)]
    id: Id,
    value: Custom,
}

#[derive(Entity)]
#[domain(id = "reference", label = "Reference", owner = Owner)]
struct Reference {
    #[domain(identity)]
    id: Id,
    value: &'static str,
}

#[derive(Entity)]
#[domain(id = "array", label = "Array", owner = Owner)]
struct Array {
    #[domain(identity)]
    id: Id,
    value: [u8; 4],
}

#[derive(Entity)]
#[domain(id = "tuple", label = "Tuple", owner = Owner)]
struct Tuple {
    #[domain(identity)]
    id: Id,
    value: (u8, u8),
}

#[derive(Entity)]
#[domain(id = "map", label = "Map", owner = Owner)]
struct Map {
    #[domain(identity)]
    id: Id,
    value: std::collections::HashMap<String, String>,
}

#[derive(Entity)]
#[domain(id = "alias", label = "Alias", owner = Owner)]
struct NonCanonical {
    #[domain(identity)]
    id: Id,
    value: collections::Vec<u8>,
}

#[derive(Entity)]
#[domain(id = "malformed", label = "Malformed", owner = Owner)]
struct MalformedWrapper {
    #[domain(identity)]
    id: Id,
    value: Option<u8, u16>,
}

fn main() {}
