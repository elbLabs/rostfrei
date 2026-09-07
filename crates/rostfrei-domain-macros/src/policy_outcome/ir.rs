use syn::{Attribute, LitStr};

pub struct Outcome {
    pub local_id: LitStr,
    pub label: LitStr,
    pub cfg_attributes: Vec<Attribute>,
}
