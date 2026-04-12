use syn::{FnArg, TraitItem};

use super::action::Action;

pub fn validate(items: &[TraitItem], actions: &mut [Action]) -> syn::Result<()> {
    validate_methods(items)?;
    for item in items {
        let TraitItem::Fn(method) = item else {
            unreachable!()
        };
        validate_receiver(&method.sig)?;
    }
    super::validation::validate_common(actions)?;
    for action in actions {
        action.signature = Some(super::signature::parse_entity(&action.syntax)?);
    }
    Ok(())
}

pub fn validate_methods(items: &[TraitItem]) -> syn::Result<()> {
    for item in items {
        let TraitItem::Fn(method) = item else {
            unreachable!()
        };
        if let Some(default) = &method.default {
            return Err(syn::Error::new_spanned(
                default,
                "domain action contract methods cannot have default bodies",
            ));
        }
    }
    Ok(())
}

fn validate_receiver(signature: &syn::Signature) -> syn::Result<()> {
    let Some(first) = signature.inputs.first() else {
        return Err(syn::Error::new_spanned(
            &signature.ident,
            "domain action contract methods require an &self or &mut self receiver",
        ));
    };
    let FnArg::Receiver(receiver) = first else {
        return Err(syn::Error::new_spanned(
            first,
            "domain action contract methods require an &self or &mut self receiver",
        ));
    };
    if receiver.colon_token.is_some() {
        return Err(syn::Error::new_spanned(
            receiver,
            "domain action contract methods do not support typed receivers",
        ));
    }
    let Some((_, lifetime)) = &receiver.reference else {
        return Err(syn::Error::new_spanned(
            receiver,
            "domain action contract methods require an &self or &mut self receiver",
        ));
    };
    if let Some(lifetime) = lifetime {
        return Err(syn::Error::new_spanned(
            lifetime,
            "domain action contract receivers cannot have an explicit lifetime",
        ));
    }
    Ok(())
}
