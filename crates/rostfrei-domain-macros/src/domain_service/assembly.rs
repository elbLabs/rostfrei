use proc_macro2::TokenStream;
use syn::{Ident, Path};

use super::{attributes::Attributes, domain_service_type};

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    domain_service_type::assemble(domain_path, name, attributes)
}
