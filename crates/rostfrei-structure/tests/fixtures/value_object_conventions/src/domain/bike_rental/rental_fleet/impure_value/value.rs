#[derive(ValueObject)]
#[domain(id = "impure-value", label = "Impure value")]
pub struct ImpureValue;

impl OtherValue {
    pub fn helper(&self) {}
}
