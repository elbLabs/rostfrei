use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Meta, Token};

pub fn collect(attributes: &[Attribute]) -> Vec<Attribute> {
    attributes.iter().filter_map(relevant_attribute).collect()
}

fn relevant_attribute(attribute: &Attribute) -> Option<Attribute> {
    if attribute.path().is_ident("cfg") {
        return Some(attribute.clone());
    }
    relevant_cfg_attr(&attribute.meta).map(|meta| syn::parse_quote!(#[#meta]))
}

fn relevant_cfg_attr(meta: &Meta) -> Option<Meta> {
    let Meta::List(list) = meta else {
        return None;
    };
    if !list.path.is_ident("cfg_attr") {
        return None;
    }
    let arguments = syn::parse2::<CfgAttrArguments>(list.tokens.clone()).ok()?;
    let attributes: Vec<_> = arguments
        .attributes
        .iter()
        .filter_map(|attribute| {
            attribute
                .path()
                .is_ident("cfg")
                .then(|| attribute.clone())
                .or_else(|| relevant_cfg_attr(attribute))
        })
        .collect();
    if attributes.is_empty() {
        return None;
    }
    let predicate = arguments.predicate;
    Some(syn::parse_quote!(cfg_attr(#predicate, #(#attributes),*)))
}

struct CfgAttrArguments {
    predicate: Meta,
    attributes: Punctuated<Meta, Token![,]>,
}

impl Parse for CfgAttrArguments {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let predicate = input.parse()?;
        input.parse::<Token![,]>()?;
        let attributes = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;
        Ok(Self {
            predicate,
            attributes,
        })
    }
}
