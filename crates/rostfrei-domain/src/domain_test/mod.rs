mod descriptor;
mod emitter;
mod projection;
mod subject;

pub use descriptor::DomainTestDescriptor;
pub use emitter::emit_domain_test_metadata;
pub use subject::DomainTestSubject;

#[cfg(test)]
mod tests;
