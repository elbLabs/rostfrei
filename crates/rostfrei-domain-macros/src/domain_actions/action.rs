use syn::LitStr;

use super::signature::ParsedSignature;

pub struct Action {
    pub id: LitStr,
    pub label: LitStr,
    pub syntax: syn::Signature,
    pub signature: Option<ParsedSignature>,
}
