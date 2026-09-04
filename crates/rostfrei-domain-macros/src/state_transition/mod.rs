mod assembly;
mod attributes;
mod collection;
mod edge_attribute;
mod expand;
mod input;
mod ir;
mod validation;

pub use expand::expand;

#[cfg(test)]
mod tests;
