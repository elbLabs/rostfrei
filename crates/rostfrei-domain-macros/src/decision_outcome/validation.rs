use std::collections::HashSet;

use super::ir::Outcome;

pub fn validate(outcomes: &[Outcome]) -> syn::Result<()> {
    let mut ids = HashSet::new();
    for outcome in outcomes {
        crate::helper::id::validate(&outcome.local_id)?;
        crate::helper::label::validate(&outcome.label)?;
        if !ids.insert(outcome.local_id.value()) {
            return Err(syn::Error::new(
                outcome.local_id.span(),
                "duplicate outcome local id",
            ));
        }
    }
    Ok(())
}
