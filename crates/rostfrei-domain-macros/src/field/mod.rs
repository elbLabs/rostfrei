mod assembly;
mod ir;
mod naming;
mod role;
mod scalar;
mod shape;

pub use assembly::{assemble_assertions_with_path, assemble_descriptors_with_path};
pub use ir::Wrapper;
pub use ir::{Field, Role};
pub use naming::extract;
