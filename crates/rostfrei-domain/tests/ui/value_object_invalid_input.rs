use domain::ValueObject;

#[derive(ValueObject)]
#[domain(id = "empty", label = "Empty")]
enum Empty {}

#[derive(ValueObject)]
#[domain(id = "discriminant", label = "Discriminant")]
enum ExplicitDiscriminant {
    Value = 1,
}

#[derive(ValueObject)]
#[domain(id = "variant-attribute", label = "Variant attribute")]
enum VariantAttribute {
    #[domain(id = "value")]
    Value,
}

#[derive(ValueObject)]
#[domain(id = "storage", label = "Storage")]
union Storage {
    integer: u64,
    decimal: f64,
}

#[derive(ValueObject)]
#[domain(id = "generic", label = "Generic")]
struct Generic<T>(T);

#[derive(ValueObject)]
#[domain(id = "generic-enum", label = "Generic enum")]
enum GenericEnum<T> {
    Value(T),
}

fn main() {}
