use rostfrei_domain::ValueObject;

#[derive(ValueObject)]
#[domain(id = "empty", label = "Empty", owner = Owner)]
enum Empty {}

#[derive(ValueObject)]
#[domain(id = "discriminant", label = "Discriminant", owner = Owner)]
enum ExplicitDiscriminant {
    Value = 1,
}

#[derive(ValueObject)]
#[domain(id = "variant-attribute", label = "Variant attribute", owner = Owner)]
enum VariantAttribute {
    #[domain(id = "value")]
    Value,
}

#[derive(ValueObject)]
#[domain(id = "storage", label = "Storage", owner = Owner)]
union Storage {
    integer: u64,
    decimal: f64,
}

#[derive(ValueObject)]
#[domain(id = "generic", label = "Generic", owner = Owner)]
struct Generic<T>(T);

#[derive(ValueObject)]
#[domain(id = "generic-enum", label = "Generic enum", owner = Owner)]
enum GenericEnum<T> {
    Value(T),
}

fn main() {}
