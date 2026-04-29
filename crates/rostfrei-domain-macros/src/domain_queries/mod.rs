mod arguments;
mod assembly;
mod attributes;
mod expand;
mod input;
mod signature;
mod validation;

pub use expand::expand;

#[cfg(test)]
mod tests;
