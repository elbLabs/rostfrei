mod assembly;
mod attributes;
mod collection;
mod expand;
mod input;
mod ir;
mod lifecycle_attribute;
mod state_attribute;
mod validation;

pub use expand::expand;

#[cfg(test)]
mod tests;
