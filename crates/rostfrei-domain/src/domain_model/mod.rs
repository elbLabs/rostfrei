mod entity_projection;
mod error;
mod field_projection;
mod field_reference_collection;
mod field_reference_validation;
mod id_projection;
mod projection;

pub use error::{DomainModelError, DomainModelReference};
pub use projection::DomainModelBuilder;

#[doc(hidden)]
pub fn try_build(
    build: impl FnOnce(&mut DomainModelBuilder) -> Result<(), DomainModelError>,
) -> Result<serde_json::Value, DomainModelError> {
    let mut builder = DomainModelBuilder::new();
    build(&mut builder)?;
    builder.finish()
}
