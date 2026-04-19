use syn::{LitStr, Path};

use super::signature::ParsedSignature;

pub struct Action {
    pub id: LitStr,
    pub label: LitStr,
    pub raises: Vec<Path>,
    pub syntax: syn::Signature,
    pub signature: Option<ParsedSignature>,
}
