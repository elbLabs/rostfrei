use proc_macro2::TokenStream;
use syn::{Ident, Path};

use super::{aggregate_type, attributes::Attributes};

pub fn assemble(domain_path: &Path, name: &Ident, attributes: &Attributes) -> TokenStream {
    aggregate_type::assemble(domain_path, name, attributes)
}
