use super::DecisionInputDescriptor;

pub trait DecisionInputType: 'static {
    const DESCRIPTOR: DecisionInputDescriptor;
}
