use syn::TraitItem;

use super::action::Action;

pub fn validate(items: &[TraitItem], actions: &mut [Action]) -> syn::Result<()> {
    super::trait_validation::validate_methods(items)?;
    super::validation::validate_common(actions)?;
    for action in actions {
        action.signature = Some(super::signature::parse_aggregate(&action.syntax)?);
    }
    Ok(())
}

pub fn validate_instance(items: &[TraitItem], actions: &mut [Action]) -> syn::Result<()> {
    super::trait_validation::validate_methods(items)?;
    super::validation::validate_common(actions)?;
    for action in actions {
        action.signature = Some(super::signature::parse_aggregate_instance(&action.syntax)?);
    }
    Ok(())
}
