mod assembly;
mod ir;
mod naming;
mod role;
mod scalar;
mod shape;

pub use assembly::{assemble_assertions, assemble_descriptors, assemble_scalar};
pub use ir::{Field, Role};
pub use naming::extract;
pub use scalar::classify as classify_scalar;
