mod action_projection;
mod action_reference_validation;
mod decision_projection;
mod decision_reference_validation;
mod entity_projection;
mod error;
mod field_projection;
mod field_reference_collection;
mod field_reference_validation;
mod id_projection;
mod projection;
mod value_object_projection;

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
