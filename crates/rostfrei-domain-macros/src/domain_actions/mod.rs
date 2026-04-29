mod action;
mod action_reference;
mod aggregate_instance_assembly;
mod aggregate_trait_assembly;
mod aggregate_trait_validation;
mod contract_arguments;
mod contract_trait_assembly;
mod domain_service_trait_assembly;
mod domain_service_trait_validation;
mod expand;
mod public_trait_input;
mod signature;
mod trait_assembly;
mod trait_attributes;
mod trait_input;
mod trait_validation;
mod validation;

pub use expand::expand;

#[cfg(test)]
mod tests;
